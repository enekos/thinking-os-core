//! One terminal on screen: a [`Vt`] fed by a host stream, painted as a cell
//! grid, with keys, mouse, selection, scrollback and search. Which panes
//! exist and where is `term_panes.rs`; the host connection is `terms.rs`.
//!
//! The host keeps the program and its output; this is a viewer. It answers
//! the program's terminal queries (the host does not), except those found in
//! a replay, which were answered when they were first asked.

use std::collections::HashMap;
use std::sync::Arc;

use artalde_core::command::Command;
use artalde_core::theme::Theme;
use artalde_host::{Client, Exit, Stream};
use artalde_vt::{
    Attrs, Cell, Colors, CursorStyle, Event, Frame, Geometry, Key, KeyAction, KeyInput, Line, Mods,
    MouseAction, MouseButton, MouseInput, Options, Rgb, Scroll, SearchStatus, SelectPoint,
    SelectStep, Underline, Vt,
};
use eframe::egui::{
    self, Color32, CornerRadius, FontId, Galley, Pos2, Rect, Sense, Stroke, pos2,
    text::{LayoutJob, TextFormat},
    vec2,
};

use crate::icons::Icon;
use crate::kit;
use crate::style::{self, tint};
use crate::term_sprites;

/// Space between the pane's edge and the grid.
pub const PAD: f32 = 6.0;
/// A resize reaches the host once the pane has held still this long.
const RESIZE_SETTLE: f64 = 0.07;
/// Bytes of output applied per frame before yielding to paint.
const PUMP_BUDGET: usize = 1 << 20;
/// How a replay starts: the host resets the viewer before repeating history.
const REPLAY: &[u8] = b"\x18\x1bc";
const BLINK_SECS: f64 = 0.6;
const SCROLLBACK_BYTES: usize = 8 << 20;
const RETRY_SECS: f64 = 2.0;

/// What a pane hands its terminal each frame.
#[derive(Clone, Copy)]
pub struct Env<'a> {
    pub client: Option<&'a Client>,
    pub theme: &'a Theme,
    pub font_size: f32,
    pub option_as_alt: bool,
    /// This is the pane keys should go to.
    pub focused: bool,
    pub now: f64,
    /// `artalde drive`: a key press with no text event still types.
    pub synthesize_text: bool,
}

/// The well terminals sit in.
/// The id a terminal's surface holds the keyboard under.
pub fn widget_id(terminal: u32) -> egui::Id {
    egui::Id::unique(("artalde-term", terminal))
}

pub fn background(theme: &Theme) -> Color32 {
    style::c(theme.background)
}

#[derive(Clone, PartialEq)]
struct Metrics {
    font: FontId,
    ppp: f32,
    cell_w: f32,
    cell_h: f32,
    /// Where a galley sits inside its row.
    text_dy: f32,
}

impl Metrics {
    fn geometry(&self) -> Geometry {
        Geometry {
            cell_w: self.cell_w,
            cell_h: self.cell_h,
            pad_x: PAD,
            pad_y: PAD,
        }
    }
}

struct Run {
    col: u16,
    /// Cells the galley is centred in; 0 for a run laid from `col` onward.
    span: u16,
    bold: bool,
    galley: Arc<Galley>,
}

struct Deco {
    start: u16,
    end: u16,
    underline: Underline,
    strike: bool,
    overline: bool,
    color: Color32,
}

#[derive(Default)]
struct Row {
    bgs: Vec<(u16, u16, Color32)>,
    runs: Vec<Run>,
    /// Characters drawn as shapes: column, character, colour.
    sprites: Vec<(u16, char, Color32)>,
    decos: Vec<Deco>,
}

struct CachedRow {
    line: Arc<Line>,
    row: Row,
}

#[derive(Clone, Copy)]
struct PendingKey {
    key: Key,
    mods: Mods,
    unshifted: Option<char>,
    action: KeyAction,
}

/// Search of the scrollback.
#[derive(Debug, Default)]
pub struct Find {
    pub query: String,
    applied: String,
    status: SearchStatus,
    grab_focus: bool,
}

pub struct TermView {
    id: u32,
    vt: Vt,
    stream: Option<Stream>,
    /// How the program ended, once it has.
    pub ended: Option<Exit>,
    /// Why the terminal cannot be shown.
    pub error: Option<String>,
    retry_at: f64,
    pub last_drawn: f64,
    grid: (u16, u16),
    host_grid: (u16, u16),
    resize_at: Option<f64>,
    metrics: Option<Metrics>,
    colors: Option<Colors>,
    frame: Option<Frame>,
    rows: Vec<Option<CachedRow>>,
    mono: HashMap<char, bool>,
    pending: Option<PendingKey>,
    scroll_acc: f32,
    selecting: bool,
    held: Option<MouseButton>,
    was_focused: bool,
    blink_from: f64,
    find: Option<Find>,
}

impl std::fmt::Debug for TermView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TermView")
            .field("id", &self.id)
            .field("grid", &self.grid)
            .field("attached", &self.stream.is_some())
            .finish_non_exhaustive()
    }
}

impl TermView {
    pub fn new(id: u32) -> Result<TermView, String> {
        Ok(TermView {
            id,
            vt: new_vt(80, 24, None)?,
            stream: None,
            ended: None,
            error: None,
            retry_at: 0.0,
            last_drawn: 0.0,
            grid: (80, 24),
            host_grid: (0, 0),
            resize_at: None,
            metrics: None,
            colors: None,
            frame: None,
            rows: Vec::new(),
            mono: HashMap::new(),
            pending: None,
            scroll_acc: 0.0,
            selecting: false,
            held: None,
            was_focused: false,
            blink_from: 0.0,
            find: None,
        })
    }

    /// Let go of the host stream; the next draw attaches again.
    pub fn detach(&mut self) {
        self.stream = None;
    }

    /// Bytes for the program, as if typed. `false` when nothing is attached.
    pub fn write(&self, bytes: &[u8]) -> bool {
        match &self.stream {
            Some(s) if !bytes.is_empty() => s.write(bytes).is_ok(),
            _ => false,
        }
    }

    fn send(&self, bytes: &[u8]) {
        if !bytes.is_empty() {
            self.write(bytes);
        }
    }

    pub fn open_find(&mut self) {
        let find = self.find.get_or_insert_with(Find::default);
        find.grab_focus = true;
    }

    pub fn copy_selection(&mut self, ctx: &egui::Context) {
        if let Some(text) = self.vt.selection_text().filter(|t| !t.is_empty()) {
            ctx.copy_text(text);
        }
    }

    /// Draw the terminal in `rect` and take its input. Returns whether the
    /// pointer was pressed on it.
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        rect: Rect,
        env: &Env<'_>,
        out: &mut Vec<Command>,
    ) -> bool {
        self.last_drawn = env.now;
        let m = self.measure(ui, env.font_size);
        self.recolor(env.theme);

        let cols = (((rect.width() - 2.0 * PAD) / m.cell_w).floor() as i32).clamp(2, 1000) as u16;
        let rows = (((rect.height() - 2.0 * PAD) / m.cell_h).floor() as i32).clamp(1, 500) as u16;
        self.attach(env, cols, rows);
        self.resize(ui, env.now, &m, cols, rows);
        self.pump(ui.ctx());

        let id = widget_id(self.id);
        let resp = ui.interact(rect, id, Sense::click_and_drag());
        let pressed = resp.contains_pointer() && ui.input(|i| i.pointer.any_pressed());
        if pressed || env.focused && ui.memory(|mem| mem.focused().is_none()) {
            resp.request_focus();
        }
        let focused = ui.memory(|mem| mem.has_focus(id));
        if focused {
            ui.memory_mut(|mem| {
                mem.set_focus_lock_filter(
                    id,
                    egui::EventFilter {
                        tab: true,
                        horizontal_arrows: true,
                        vertical_arrows: true,
                        escape: true,
                    },
                );
            });
        }
        if focused != self.was_focused {
            self.was_focused = focused;
            self.blink_from = env.now;
            let bytes = self.vt.focus(focused);
            self.send(&bytes);
        }
        if focused {
            self.keyboard(ui, env);
        }
        self.pointer(ui, &resp, rect, &m, out);
        self.menu(&resp, env, out);

        let find_changed = self.search(ui);
        if self.frame.is_none() || self.vt.changed() || find_changed {
            self.frame = Some(self.vt.frame());
        }
        self.rebuild(ui, &m);
        self.paint(ui, rect, &m, env, focused, resp.hovered());
        self.find_bar(ui, rect, env);
        pressed
    }

    fn measure(&mut self, ui: &egui::Ui, size: f32) -> Metrics {
        let ppp = ui.ctx().pixels_per_point();
        if let Some(m) = &self.metrics
            && m.font.size == size
            && m.ppp == ppp
        {
            return m.clone();
        }
        let font = FontId::monospace(size);
        let (w, h) = ui.fonts_mut(|f| (f.glyph_width(&font, 'M'), f.row_height(&font)));
        let cell_h = (h * ppp).ceil() / ppp;
        let m = Metrics {
            font,
            ppp,
            cell_w: w.max(1.0),
            cell_h: cell_h.max(1.0),
            text_dy: (cell_h - h) / 2.0,
        };
        self.metrics = Some(m.clone());
        self.rows.clear();
        self.mono.clear();
        m
    }

    fn recolor(&mut self, theme: &Theme) {
        let colors = colors(theme);
        if self.colors.as_ref() != Some(&colors) {
            self.vt.set_colors(&colors);
            self.colors = Some(colors);
            self.rows.clear();
        }
    }

    fn attach(&mut self, env: &Env<'_>, cols: u16, rows: u16) {
        if self.stream.is_some() || self.ended.is_some() || env.now < self.retry_at {
            return;
        }
        let Some(client) = env.client else { return };
        match client.attach(self.id, cols, rows, true) {
            Ok(stream) => match new_vt(cols, rows, self.colors.as_ref()) {
                Ok(vt) => {
                    self.vt = vt;
                    self.stream = Some(stream);
                    self.grid = (cols, rows);
                    self.host_grid = (cols, rows);
                    self.resize_at = None;
                    self.frame = None;
                    self.rows.clear();
                    self.error = None;
                    self.was_focused = false;
                }
                Err(e) => self.error = Some(e),
            },
            Err(e) => {
                self.error = Some(e.to_string());
                self.retry_at = env.now + RETRY_SECS;
            }
        }
    }

    fn resize(&mut self, ui: &egui::Ui, now: f64, m: &Metrics, cols: u16, rows: u16) {
        if (cols, rows) != self.grid {
            self.grid = (cols, rows);
            self.vt.resize(
                cols,
                rows,
                (m.cell_w * m.ppp).round() as u32,
                (m.cell_h * m.ppp).round() as u32,
            );
            self.resize_at = Some(now + RESIZE_SETTLE);
        }
        let Some(at) = self.resize_at else { return };
        if now < at {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_secs_f64(at - now));
            return;
        }
        self.resize_at = None;
        if self.host_grid != self.grid
            && let Some(s) = &self.stream
            && s.resize(
                cols,
                rows,
                (m.cell_w * m.ppp).round() as u16,
                (m.cell_h * m.ppp).round() as u16,
            )
            .is_ok()
        {
            self.host_grid = self.grid;
        }
    }

    /// Apply what the host sent since the last frame.
    fn pump(&mut self, ctx: &egui::Context) {
        let Some(stream) = &mut self.stream else {
            return;
        };
        let mut budget = PUMP_BUDGET;
        loop {
            let Some(chunk) = stream.try_next() else {
                if let Some(exit) = stream.ended() {
                    match exit {
                        // The host went away, not the program: attach again.
                        Exit::Disconnected => {}
                        other => self.ended = Some(other),
                    }
                    self.stream = None;
                }
                break;
            };
            self.vt.write(&chunk);
            let replies = self.vt.take_replies();
            let events = self.vt.take_events();
            // A replay repeats old queries; they were answered back then.
            if !chunk.starts_with(REPLAY) {
                if !replies.is_empty() && stream.write(&replies).is_err() {
                    break;
                }
                for ev in events {
                    if let Event::Clipboard(text) = ev {
                        ctx.copy_text(text);
                    }
                }
            }
            budget = budget.saturating_sub(chunk.len());
            if budget == 0 {
                ctx.request_repaint();
                break;
            }
        }
    }

    fn keyboard(&mut self, ui: &egui::Ui, env: &Env<'_>) {
        let events = ui.input(|i| i.events.clone());
        let mut typed = false;
        for ev in &events {
            match ev {
                egui::Event::Key {
                    key,
                    physical_key,
                    pressed,
                    repeat,
                    modifiers,
                } => {
                    // ⌘ chords belong to the window.
                    if modifiers.mac_cmd {
                        if *pressed && *key == egui::Key::A {
                            self.vt.select_all();
                        }
                        continue;
                    }
                    let mut mods = Mods::NONE;
                    if modifiers.shift {
                        mods |= Mods::SHIFT;
                    }
                    if modifiers.ctrl {
                        mods |= Mods::CTRL;
                    }
                    if modifiers.alt {
                        mods |= Mods::ALT;
                    }
                    if *pressed
                        && modifiers.shift
                        && !self.vt.alt_screen()
                        && matches!(key, egui::Key::PageUp | egui::Key::PageDown)
                    {
                        let by = if *key == egui::Key::PageUp { -1 } else { 1 };
                        self.vt.scroll(Scroll::Pages(by));
                        continue;
                    }
                    let chord = modifiers.ctrl || (modifiers.alt && env.option_as_alt);
                    // Text comes from the layout, so the key is named by
                    // where it sits; a chord means the letter it is labelled.
                    let named = if chord {
                        *key
                    } else {
                        physical_key.unwrap_or(*key)
                    };
                    let Some((vkey, prints)) = vt_key(named) else {
                        continue;
                    };
                    let action = match (*pressed, *repeat) {
                        (false, _) => KeyAction::Release,
                        (true, true) => KeyAction::Repeat,
                        (true, false) => KeyAction::Press,
                    };
                    let k = PendingKey {
                        key: vkey,
                        mods,
                        unshifted: unshifted(vkey),
                        action,
                    };
                    if !*pressed {
                        if self.vt.kitty_keyboard_flags() != 0 {
                            let bytes = self.encode(k, None, env);
                            self.send(&bytes);
                        }
                        continue;
                    }
                    self.flush_pending(env);
                    typed = true;
                    if prints && !chord {
                        self.pending = Some(k);
                    } else {
                        let bytes = self.encode(k, None, env);
                        self.send(&bytes);
                    }
                }
                egui::Event::Text(text) => {
                    if ui.input(|i| i.modifiers.alt) && env.option_as_alt {
                        continue;
                    }
                    typed = true;
                    match self.pending.take() {
                        Some(k) => {
                            let bytes = self.encode(k, Some(text), env);
                            self.send(&bytes);
                        }
                        None => self.send(text.as_bytes()),
                    }
                }
                egui::Event::Ime(egui::ImeEvent::Commit(text)) => {
                    typed = true;
                    self.pending = None;
                    self.send(text.as_bytes());
                }
                egui::Event::Paste(text) => {
                    typed = true;
                    let bytes = self.vt.paste(text);
                    self.send(&bytes);
                }
                egui::Event::Copy | egui::Event::Cut => self.copy_selection(ui.ctx()),
                _ => {}
            }
        }
        self.flush_pending(env);
        if typed {
            self.vt.scroll(Scroll::Bottom);
            self.vt.clear_selection();
            self.blink_from = env.now;
        }
    }

    /// A printing key whose text never came: a dead key in a window, a
    /// scripted key in a drive.
    fn flush_pending(&mut self, env: &Env<'_>) {
        let Some(k) = self.pending.take() else { return };
        if !env.synthesize_text {
            return;
        }
        let Some(c) = k.unshifted else { return };
        let c = if k.mods.contains(Mods::SHIFT) {
            c.to_ascii_uppercase()
        } else {
            c
        };
        let bytes = self.encode(k, Some(c.encode_utf8(&mut [0; 4])), env);
        self.send(&bytes);
    }

    fn encode(&mut self, k: PendingKey, text: Option<&str>, env: &Env<'_>) -> Vec<u8> {
        let mut consumed = Mods::NONE;
        if text.is_some() {
            if k.mods.contains(Mods::SHIFT) {
                consumed |= Mods::SHIFT;
            }
            if k.mods.contains(Mods::ALT) && !env.option_as_alt {
                consumed |= Mods::ALT;
            }
        }
        self.vt.key(&KeyInput {
            key: k.key,
            mods: k.mods,
            consumed,
            text,
            unshifted: k.unshifted,
            action: k.action,
            option_as_alt: env.option_as_alt,
        })
    }

    fn cell_at(&self, rect: Rect, m: &Metrics, pos: Pos2) -> (u16, u16) {
        let col = ((pos.x - rect.min.x - PAD) / m.cell_w).floor();
        let row = ((pos.y - rect.min.y - PAD) / m.cell_h).floor();
        (
            col.clamp(0.0, f32::from(self.grid.0.saturating_sub(1))) as u16,
            row.clamp(0.0, f32::from(self.grid.1.saturating_sub(1))) as u16,
        )
    }

    fn pointer(
        &mut self,
        ui: &egui::Ui,
        resp: &egui::Response,
        rect: Rect,
        m: &Metrics,
        out: &mut Vec<Command>,
    ) {
        let (mods, shift, cmd, alt) = ui.input(|i| {
            let md = i.modifiers;
            let mut mods = Mods::NONE;
            if md.shift {
                mods |= Mods::SHIFT;
            }
            if md.ctrl {
                mods |= Mods::CTRL;
            }
            if md.alt {
                mods |= Mods::ALT;
            }
            (mods, md.shift, md.command, md.alt)
        });
        let hover = resp.contains_pointer();
        let tracking = self.vt.mouse_tracking() && !shift;

        // The wheel: the program's if it tracks the mouse, arrow keys on an
        // alternate screen, otherwise the scrollback.
        if hover {
            let dy = ui.input(|i| i.smooth_scroll_delta.y);
            if dy != 0.0 {
                ui.input_mut(|i| i.smooth_scroll_delta = egui::Vec2::ZERO);
                self.scroll_acc += dy / m.cell_h;
                let n = self.scroll_acc.trunc();
                self.scroll_acc -= n;
                let n = n as isize;
                if n != 0 {
                    let pos = ui.input(|i| i.pointer.hover_pos()).unwrap_or(rect.center());
                    if tracking {
                        let button = if n > 0 {
                            MouseButton::WheelUp
                        } else {
                            MouseButton::WheelDown
                        };
                        for _ in 0..n.unsigned_abs().min(20) {
                            self.mouse(rect, m, MouseAction::Press, Some(button), mods, pos);
                        }
                    } else if self.vt.alt_screen() {
                        let key = if n > 0 { Key::ArrowUp } else { Key::ArrowDown };
                        for _ in 0..n.unsigned_abs().min(20) {
                            let bytes = self.vt.key(&KeyInput::press(key));
                            self.send(&bytes);
                        }
                    } else {
                        self.vt.scroll(Scroll::Lines(-n));
                    }
                }
            }
        }

        if tracking {
            self.selecting = false;
            let events = ui.input(|i| i.events.clone());
            for ev in &events {
                match ev {
                    egui::Event::PointerButton {
                        pos,
                        button,
                        pressed,
                        ..
                    } => {
                        let b = match button {
                            egui::PointerButton::Primary => MouseButton::Left,
                            egui::PointerButton::Secondary => MouseButton::Right,
                            egui::PointerButton::Middle => MouseButton::Middle,
                            _ => continue,
                        };
                        if *pressed && hover {
                            self.held = Some(b);
                            self.mouse(rect, m, MouseAction::Press, Some(b), mods, *pos);
                        } else if !*pressed && self.held == Some(b) {
                            self.held = None;
                            self.mouse(rect, m, MouseAction::Release, Some(b), mods, *pos);
                        }
                    }
                    egui::Event::PointerMoved(pos) if hover || self.held.is_some() => {
                        self.mouse(rect, m, MouseAction::Motion, self.held, mods, *pos);
                    }
                    _ => {}
                }
            }
            return;
        }
        self.held = None;

        let (press, down, release, pos) = ui.input(|i| {
            (
                i.pointer.primary_pressed(),
                i.pointer.primary_down(),
                i.pointer.primary_released(),
                i.pointer.latest_pos(),
            )
        });
        let Some(pos) = pos else { return };
        let (col, row) = self.cell_at(rect, m, pos);
        let point = SelectPoint {
            col,
            row,
            x: pos.x - rect.min.x,
            y: pos.y - rect.min.y,
            geometry: m.geometry(),
            rectangle: alt,
        };
        if press && hover {
            if cmd {
                if let Some(url) = self.link_at(col, row) {
                    out.push(Command::new("url.open", Some(&url)));
                }
                return;
            }
            self.selecting = true;
            self.vt.select(SelectStep::Press, point);
        } else if self.selecting && release {
            self.selecting = false;
            self.vt.select(SelectStep::Release, point);
        } else if self.selecting && down {
            self.vt.select(SelectStep::Drag, point);
            // Past the edge: bring more into reach.
            if pos.y < rect.min.y {
                self.vt.scroll(Scroll::Lines(-1));
                ui.ctx().request_repaint();
            } else if pos.y > rect.max.y {
                self.vt.scroll(Scroll::Lines(1));
                ui.ctx().request_repaint();
            }
        } else if !down {
            self.selecting = false;
        }
        if hover && cmd && self.link_at(col, row).is_some() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        } else if hover {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Text);
        }
    }

    fn mouse(
        &mut self,
        rect: Rect,
        m: &Metrics,
        action: MouseAction,
        button: Option<MouseButton>,
        mods: Mods,
        pos: Pos2,
    ) {
        let bytes = self.vt.mouse(&MouseInput {
            action,
            button,
            mods,
            x: pos.x - rect.min.x,
            y: pos.y - rect.min.y,
            geometry: m.geometry(),
        });
        self.send(&bytes);
    }

    fn link_at(&self, col: u16, row: u16) -> Option<String> {
        self.vt
            .link_at(col, row)
            .filter(|l| l.starts_with("http://") || l.starts_with("https://"))
            .or_else(|| url_in(&self.vt.row_text(row), usize::from(col)))
    }

    fn menu(&mut self, resp: &egui::Response, env: &Env<'_>, out: &mut Vec<Command>) {
        // A program that tracks the mouse gets its right clicks.
        if self.vt.mouse_tracking() && !resp.ctx.input(|i| i.modifiers.shift) {
            return;
        }
        let theme = env.theme;
        let has_selection = self.vt.has_selection();
        let mut copy = false;
        let mut select_all = false;
        resp.context_menu(|ui| {
            ui.set_min_width(200.0);
            ui.add_enabled_ui(has_selection, |ui| {
                if kit::menu_action(ui, theme, Icon::Copy, "Copy", Some("⌘C")).clicked() {
                    copy = true;
                    ui.close();
                }
            });
            if kit::menu_action(ui, theme, Icon::Clipboard, "Paste", Some("⌘V")).clicked() {
                ui.ctx()
                    .send_viewport_cmd(egui::ViewportCommand::RequestPaste);
                ui.close();
            }
            if kit::menu_action(ui, theme, Icon::SquareCheck, "Select all", Some("⌘A")).clicked()
            {
                select_all = true;
                ui.close();
            }
            if kit::menu_action(ui, theme, Icon::Search, "Find…", Some("⌘F")).clicked() {
                out.push(Command::new("term.find", None));
                ui.close();
            }
            kit::menu_rule(ui, theme);
            if kit::menu_action(ui, theme, Icon::SplitRight, "Split right", Some("⌘D")).clicked()
            {
                out.push(Command::new("term.split", Some("right")));
                ui.close();
            }
            if kit::menu_action(ui, theme, Icon::SplitDown, "Split down", Some("⇧⌘D")).clicked()
            {
                out.push(Command::new("term.split", Some("down")));
                ui.close();
            }
            kit::menu_rule(ui, theme);
            if kit::menu_action(ui, theme, Icon::Close, "Close pane", Some("⌘W")).clicked() {
                out.push(Command::new("term.close", None));
                ui.close();
            }
        });
        if copy {
            self.copy_selection(&resp.ctx);
        }
        if select_all {
            self.vt.select_all();
        }
    }

    /// Advance an open search; `true` when what is highlighted changed.
    fn search(&mut self, ui: &egui::Ui) -> bool {
        let Some(find) = &mut self.find else {
            return false;
        };
        let mut changed = false;
        if find.query != find.applied {
            find.applied.clone_from(&find.query);
            self.vt.set_needle(Some(&find.query));
            changed = true;
        }
        let status = self.vt.search_tick();
        if status != find.status {
            find.status = status;
            changed = true;
        }
        if !status.complete {
            ui.ctx().request_repaint();
        }
        changed
    }

    fn find_bar(&mut self, ui: &mut egui::Ui, rect: Rect, env: &Env<'_>) {
        let Some(find) = &mut self.find else { return };
        let theme = env.theme;
        let w = 320.0_f32.min(rect.width() - 2.0 * kit::S2);
        if w < 160.0 {
            return;
        }
        let bar = Rect::from_min_size(
            pos2(rect.max.x - w - kit::S2, rect.min.y + kit::S2),
            vec2(w, kit::CONTROL + 8.0),
        );
        let mut step = 0i8;
        let mut close = false;
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(bar));
        egui::Frame::new()
            .fill(style::c(theme.raised))
            .stroke(Stroke::new(1.0, style::c(theme.border)))
            .corner_radius(CornerRadius::same(kit::BUTTON_RADIUS + 2))
            .inner_margin(egui::Margin::symmetric(4, 3))
            .shadow(egui::Shadow {
                offset: [0, 4],
                blur: 14,
                spread: 0,
                color: Color32::from_black_alpha(60),
            })
            .show(&mut child, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    let count = match (find.status.selected, find.status.total) {
                        _ if find.query.is_empty() => String::new(),
                        (_, 0) => "none".to_owned(),
                        (Some(i), n) => format!("{}/{n}", i + 1),
                        (None, n) => n.to_string(),
                    };
                    let field_w =
                        (ui.available_width() - 3.0 * (kit::ICON_BUTTON + 2.0) - 44.0).max(60.0);
                    let r = kit::search(ui, theme, &mut find.query, "Find in terminal", field_w);
                    if find.grab_focus {
                        find.grab_focus = false;
                        r.request_focus();
                    }
                    let (enter, shift, escape) = ui.input(|i| {
                        (
                            i.key_pressed(egui::Key::Enter),
                            i.modifiers.shift,
                            i.key_pressed(egui::Key::Escape),
                        )
                    });
                    if r.lost_focus() && enter {
                        step = if shift { -1 } else { 1 };
                        r.request_focus();
                    }
                    if (r.has_focus() || r.lost_focus()) && escape {
                        close = true;
                    }
                    ui.allocate_ui_with_layout(
                        vec2(44.0, kit::CONTROL),
                        egui::Layout::centered_and_justified(egui::Direction::LeftToRight),
                        |ui| kit::mono(ui, &count, 10.5, style::c(theme.dim)),
                    );
                    if kit::icon_button(ui, theme, Icon::ChevronUp, "Previous match (↵)").clicked()
                    {
                        step = 1;
                    }
                    if kit::icon_button(ui, theme, Icon::ChevronDown, "Next match (⇧↵)").clicked()
                    {
                        step = -1;
                    }
                    if kit::icon_button(ui, theme, Icon::Close, "Close (esc)").clicked() {
                        close = true;
                    }
                });
            });
        if step > 0 {
            find.status = self.vt.search_next();
        } else if step < 0 {
            find.status = self.vt.search_prev();
        }
        if close {
            self.close_find();
            ui.memory_mut(|mem| {
                if let Some(id) = mem.focused() {
                    mem.surrender_focus(id);
                }
            });
        }
    }

    pub fn close_find(&mut self) {
        if self.find.take().is_some() {
            self.vt.set_needle(None);
        }
    }

    /// Bring the row cache in line with the frame.
    fn rebuild(&mut self, ui: &egui::Ui, m: &Metrics) {
        let Some(frame) = &self.frame else { return };
        self.rows.resize_with(frame.lines.len(), || None);
        let stale = frame
            .lines
            .iter()
            .zip(&self.rows)
            .any(|(line, cached)| cached.as_ref().is_none_or(|c| !Arc::ptr_eq(&c.line, line)));
        if !stale {
            return;
        }
        let fg = rgb(frame.foreground);
        let bg = rgb(frame.background);
        let rows = &mut self.rows;
        let mono = &mut self.mono;
        ui.fonts_mut(|fonts| {
            for (line, cached) in frame.lines.iter().zip(rows.iter_mut()) {
                if cached.as_ref().is_some_and(|c| Arc::ptr_eq(&c.line, line)) {
                    continue;
                }
                let mut shaper = Shaper { fonts, mono, m };
                *cached = Some(CachedRow {
                    line: Arc::clone(line),
                    row: build_row(&line.cells, m, fg, bg, &mut shaper),
                });
            }
        });
    }

    fn paint(
        &mut self,
        ui: &egui::Ui,
        rect: Rect,
        m: &Metrics,
        env: &Env<'_>,
        focused: bool,
        hovered: bool,
    ) {
        let Some(frame) = &self.frame else { return };
        let theme = env.theme;
        let p = ui.painter_at(rect);
        let snap = |v: f32| (v * m.ppp).round() / m.ppp;
        let origin = pos2(snap(rect.min.x + PAD), snap(rect.min.y + PAD));
        let x_of = |col: u16| origin.x + f32::from(col) * m.cell_w;
        let y_of = |row: usize| origin.y + row as f32 * m.cell_h;
        p.rect_filled(rect, CornerRadius::ZERO, rgb(frame.background));

        for (y, cached) in self.rows.iter().enumerate() {
            let Some(c) = cached else { continue };
            for (a, b, color) in &c.row.bgs {
                p.rect_filled(
                    Rect::from_min_max(
                        pos2(snap(x_of(*a)), y_of(y)),
                        pos2(snap(x_of(*b)), y_of(y) + m.cell_h),
                    ),
                    CornerRadius::ZERO,
                    *color,
                );
            }
        }

        if self.find.is_some() {
            let accent = style::c(theme.accent);
            for hit in self.vt.search_matches() {
                let r = Rect::from_min_max(
                    pos2(snap(x_of(hit.col_start)), y_of(usize::from(hit.row))),
                    pos2(
                        snap(x_of(hit.col_end)),
                        y_of(usize::from(hit.row)) + m.cell_h,
                    ),
                );
                if hit.selected {
                    p.rect_filled(r, CornerRadius::same(2), accent.gamma_multiply(0.55));
                    p.rect_stroke(
                        r,
                        CornerRadius::same(2),
                        Stroke::new(1.0, accent),
                        egui::StrokeKind::Inside,
                    );
                } else {
                    p.rect_filled(r, CornerRadius::same(2), accent.gamma_multiply(0.22));
                }
            }
        }

        let line_w = (m.cell_h / 16.0).round().max(1.0);
        for (y, cached) in self.rows.iter().enumerate() {
            let Some(c) = cached else { continue };
            let top = y_of(y);
            for &(col, ch, color) in &c.row.sprites {
                let cell = Rect::from_min_max(
                    pos2(snap(x_of(col)), snap(top)),
                    pos2(snap(x_of(col + 1)), snap(top + m.cell_h)),
                );
                term_sprites::paint(&p, cell, ch, color, line_w);
            }
            for run in &c.row.runs {
                let x = if run.span == 0 {
                    x_of(run.col)
                } else {
                    x_of(run.col) + (f32::from(run.span) * m.cell_w - run.galley.size().x) / 2.0
                };
                let at = pos2(x, top + m.text_dy);
                p.galley(at, Arc::clone(&run.galley), Color32::PLACEHOLDER);
                if run.bold {
                    p.galley(
                        at + vec2(0.5, 0.0),
                        Arc::clone(&run.galley),
                        Color32::PLACEHOLDER,
                    );
                }
            }
            for d in &c.row.decos {
                let (x0, x1) = (x_of(d.start), x_of(d.end));
                let stroke = Stroke::new(line_w, d.color);
                let under = snap(top + m.cell_h - 2.0) + line_w / 2.0;
                match d.underline {
                    Underline::None => {}
                    Underline::Single => {
                        p.hline(x0..=x1, under, stroke);
                    }
                    Underline::Double => {
                        p.hline(x0..=x1, under, stroke);
                        p.hline(x0..=x1, under - 2.0 * line_w, stroke);
                    }
                    Underline::Curly => {
                        let n = ((x1 - x0) / 2.0).ceil().max(1.0) as usize;
                        let points: Vec<Pos2> = (0..=n)
                            .map(|i| {
                                let x = x0 + (x1 - x0) * i as f32 / n as f32;
                                let dy = if i % 2 == 0 { -1.0 } else { 0.5 };
                                pos2(x, under + dy)
                            })
                            .collect();
                        p.add(egui::Shape::line(points, stroke));
                    }
                    Underline::Dotted => {
                        p.extend(egui::Shape::dashed_line(
                            &[pos2(x0, under), pos2(x1, under)],
                            stroke,
                            line_w,
                            line_w * 2.0,
                        ));
                    }
                    Underline::Dashed => {
                        p.extend(egui::Shape::dashed_line(
                            &[pos2(x0, under), pos2(x1, under)],
                            stroke,
                            4.0,
                            2.0,
                        ));
                    }
                }
                if d.strike {
                    p.hline(x0..=x1, snap(top + m.cell_h * 0.55), stroke);
                }
                if d.overline {
                    p.hline(x0..=x1, snap(top) + line_w / 2.0, stroke);
                }
            }
        }

        if let Some(cur) = frame.cursor {
            let color = cur.color.map_or(style::c(theme.accent), rgb);
            let cell = Rect::from_min_size(
                pos2(snap(x_of(cur.x)), y_of(usize::from(cur.y))),
                vec2(m.cell_w, m.cell_h),
            );
            let on = !cur.blinking || !focused || {
                let phase = (env.now - self.blink_from) / BLINK_SECS;
                let left = (1.0 - phase.fract()) * BLINK_SECS;
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_secs_f64(left.max(0.02)));
                (phase as i64) % 2 == 0
            };
            if !focused || self.ended.is_some() {
                p.rect_stroke(
                    cell,
                    CornerRadius::same(1),
                    Stroke::new(1.0, color.gamma_multiply(0.7)),
                    egui::StrokeKind::Inside,
                );
            } else if on {
                match cur.style {
                    CursorStyle::Block => {
                        p.rect_filled(cell, CornerRadius::same(1), color);
                        let under = frame
                            .lines
                            .get(usize::from(cur.y))
                            .and_then(|l| l.cells.get(usize::from(cur.x)));
                        if let Some(under) = under.filter(|c| !c.text.is_empty()) {
                            let mut text = String::new();
                            under.text.push_to(&mut text);
                            let galley = p.layout_no_wrap(
                                text,
                                m.font.clone(),
                                contrast(color, rgb(frame.background), rgb(frame.foreground)),
                            );
                            let x = cell.min.x + (m.cell_w - galley.size().x).max(0.0) / 2.0;
                            p.galley(
                                pos2(x, cell.min.y + m.text_dy),
                                galley,
                                Color32::PLACEHOLDER,
                            );
                        }
                    }
                    CursorStyle::HollowBlock => {
                        p.rect_stroke(
                            cell,
                            CornerRadius::same(1),
                            Stroke::new(1.0, color),
                            egui::StrokeKind::Inside,
                        );
                    }
                    CursorStyle::Bar => {
                        p.rect_filled(
                            Rect::from_min_size(cell.min, vec2(2.0_f32.max(line_w), m.cell_h)),
                            CornerRadius::ZERO,
                            color,
                        );
                    }
                    CursorStyle::Underline => {
                        p.rect_filled(
                            Rect::from_min_max(pos2(cell.min.x, cell.max.y - 2.0), cell.max),
                            CornerRadius::ZERO,
                            color,
                        );
                    }
                }
            }
        }

        let sb = frame.scrollbar;
        if sb.total > sb.len && (!frame.at_bottom || hovered) {
            let track = rect.height() - 4.0;
            let h = (track * sb.len as f32 / sb.total as f32)
                .max(24.0)
                .min(track);
            let span = (sb.total - sb.len) as f32;
            let y = rect.min.y + 2.0 + (track - h) * (sb.offset as f32 / span).clamp(0.0, 1.0);
            p.rect_filled(
                Rect::from_min_size(pos2(rect.max.x - 5.0, y), vec2(3.0, h)),
                CornerRadius::same(2),
                tint(
                    rgb(frame.background),
                    rgb(frame.foreground),
                    if frame.at_bottom { 0.18 } else { 0.38 },
                ),
            );
        }
    }
}

fn new_vt(cols: u16, rows: u16, colors: Option<&Colors>) -> Result<Vt, String> {
    Vt::new(&Options {
        cols,
        rows,
        scrollback_bytes: SCROLLBACK_BYTES,
        colors: colors.cloned(),
    })
    .map_err(|e| e.to_string())
}

fn rgb(c: Rgb) -> Color32 {
    Color32::from_rgb(c.r, c.g, c.b)
}

fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let f = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t).round() as u8;
    Color32::from_rgb(f(a.r(), b.r()), f(a.g(), b.g()), f(a.b(), b.b()))
}

/// Whichever of `a` and `b` reads better on `on`.
fn contrast(on: Color32, a: Color32, b: Color32) -> Color32 {
    let luma =
        |c: Color32| 0.299 * f32::from(c.r()) + 0.587 * f32::from(c.g()) + 0.114 * f32::from(c.b());
    if (luma(on) - luma(a)).abs() >= (luma(on) - luma(b)).abs() {
        a
    } else {
        b
    }
}

/// The terminal palette for a theme: its own text, well and accent, and one
/// of two fixed ANSI sets by whether it is dark.
pub fn colors(theme: &Theme) -> Colors {
    const DARK: [(u8, u8, u8); 16] = [
        (0x45, 0x4b, 0x57),
        (0xf0, 0x71, 0x78),
        (0x8b, 0xd4, 0x9c),
        (0xf2, 0xcc, 0x60),
        (0x6c, 0xa8, 0xf5),
        (0xc7, 0x92, 0xea),
        (0x5c, 0xcf, 0xe6),
        (0xc8, 0xcc, 0xd4),
        (0x6b, 0x72, 0x80),
        (0xff, 0x8b, 0x92),
        (0xa5, 0xe8, 0xb4),
        (0xff, 0xdb, 0x80),
        (0x8f, 0xc0, 0xff),
        (0xdd, 0xb0, 0xf6),
        (0x82, 0xe2, 0xf2),
        (0xf2, 0xf4, 0xf8),
    ];
    const LIGHT: [(u8, u8, u8); 16] = [
        (0x2b, 0x2f, 0x38),
        (0xc9, 0x3c, 0x37),
        (0x2f, 0x8f, 0x4e),
        (0xa8, 0x71, 0x00),
        (0x2f, 0x6f, 0xd6),
        (0x8b, 0x4f, 0xc7),
        (0x0e, 0x86, 0x96),
        (0x9a, 0xa1, 0xaf),
        (0x5c, 0x63, 0x70),
        (0xe0, 0x52, 0x4c),
        (0x3b, 0xa5, 0x5d),
        (0xc0, 0x86, 0x0a),
        (0x4a, 0x86, 0xe8),
        (0xa3, 0x67, 0xdc),
        (0x1a, 0x9f, 0xb0),
        (0x1b, 0x1d, 0x23),
    ];
    let of = |c: Color32| Rgb::new(c.r(), c.g(), c.b());
    let bg = background(theme);
    let accent = style::c(theme.accent);
    let set = if theme.dark { DARK } else { LIGHT };
    Colors {
        foreground: of(style::c(theme.text)),
        background: of(bg),
        cursor: Some(of(accent)),
        selection: Some((of(mix(bg, accent, 0.34)), None)),
        ansi: set.map(|(r, g, b)| Rgb::new(r, g, b)),
    }
}

struct RunBuilder {
    col: u16,
    bold: bool,
    job: LayoutJob,
    /// The section being gathered, and its colour and slant.
    text: String,
    format: (Color32, bool),
    /// Blanks seen since the last glyph; dropped if nothing follows.
    blanks: usize,
}

impl RunBuilder {
    fn close_section(&mut self, font: &FontId) {
        if !self.text.is_empty() {
            self.job.append(
                &self.text,
                0.0,
                TextFormat {
                    font_id: font.clone(),
                    color: self.format.0,
                    italics: self.format.1,
                    ..Default::default()
                },
            );
            self.text.clear();
        }
    }
}

/// Font access while rows are rebuilt.
struct Shaper<'a, 'f> {
    fonts: &'a mut egui::epaint::text::FontsView<'f>,
    mono: &'a mut HashMap<char, bool>,
    m: &'a Metrics,
}

impl Shaper<'_, '_> {
    /// Whether `ch` is exactly one cell wide in the terminal font, so a run
    /// of such characters lands on the grid by itself.
    fn safe(&mut self, ch: char) -> bool {
        ch.is_ascii_graphic()
            || *self.mono.entry(ch).or_insert_with(|| {
                self.fonts.has_glyph(&self.m.font, ch)
                    && (self.fonts.glyph_width(&self.m.font, ch) - self.m.cell_w).abs() < 0.01
            })
    }
}

fn finish(open: &mut Option<RunBuilder>, row: &mut Row, shaper: &mut Shaper<'_, '_>) {
    if let Some(mut run) = open.take() {
        run.close_section(&shaper.m.font);
        row.runs.push(Run {
            col: run.col,
            span: 0,
            bold: run.bold,
            galley: shaper.fonts.layout_job(run.job),
        });
    }
}

fn job() -> LayoutJob {
    let mut job = LayoutJob::default();
    job.wrap.max_width = f32::INFINITY;
    job.break_on_newline = false;
    job.round_output_to_gui = false;
    job
}

fn build_row(
    cells: &[Cell],
    m: &Metrics,
    fg_default: Color32,
    bg_default: Color32,
    shaper: &mut Shaper<'_, '_>,
) -> Row {
    let mut row = Row::default();
    let mut open: Option<RunBuilder> = None;
    let mut cluster = String::new();
    for (i, cell) in cells.iter().enumerate() {
        let col = i as u16;
        if let Some(bg) = cell.bg {
            let bg = rgb(bg);
            match row.bgs.last_mut() {
                Some((_, end, color)) if *end == col && *color == bg => *end = col + 1,
                _ => row.bgs.push((col, col + 1, bg)),
            }
        }
        let attrs = cell.attrs;
        if attrs.contains(Attrs::SPACER) {
            continue;
        }
        let mut fg = cell.fg.map_or(fg_default, rgb);
        if attrs.contains(Attrs::FAINT) {
            fg = mix(fg, cell.bg.map_or(bg_default, rgb), 0.45);
        }
        let strike = attrs.contains(Attrs::STRIKE);
        let overline = attrs.contains(Attrs::OVERLINE);
        if cell.underline != Underline::None || strike || overline {
            let color = cell.underline_color.map_or(fg, rgb);
            let width = if attrs.contains(Attrs::WIDE) { 2 } else { 1 };
            match row.decos.last_mut() {
                Some(d)
                    if d.end == col
                        && d.underline == cell.underline
                        && d.strike == strike
                        && d.overline == overline
                        && d.color == color =>
                {
                    d.end = col + width;
                }
                _ => row.decos.push(Deco {
                    start: col,
                    end: col + width,
                    underline: cell.underline,
                    strike,
                    overline,
                    color,
                }),
            }
        }
        let single = match (cell.text.cluster(), cell.text.first()) {
            (Some(_), _) => None,
            (None, c) => c,
        };
        if cell.text.is_empty() || attrs.contains(Attrs::INVISIBLE) || single == Some(' ') {
            if let Some(run) = &mut open {
                run.blanks += 1;
            }
            continue;
        }
        if let Some(c) = single.filter(|c| !attrs.contains(Attrs::WIDE) && term_sprites::covers(*c))
        {
            row.sprites.push((col, c, fg));
            if let Some(run) = &mut open {
                run.blanks += 1;
            }
            continue;
        }
        let bold = attrs.contains(Attrs::BOLD);
        let italic = attrs.contains(Attrs::ITALIC);
        match single.filter(|c| !attrs.contains(Attrs::WIDE) && shaper.safe(*c)) {
            Some(c) => {
                if open.as_ref().is_some_and(|r| r.bold != bold) {
                    finish(&mut open, &mut row, shaper);
                }
                let run = open.get_or_insert_with(|| RunBuilder {
                    col,
                    bold,
                    job: job(),
                    text: String::new(),
                    format: (fg, italic),
                    blanks: 0,
                });
                run.text.extend(std::iter::repeat_n(' ', run.blanks));
                run.blanks = 0;
                if run.format != (fg, italic) {
                    run.close_section(&m.font);
                    run.format = (fg, italic);
                }
                run.text.push(c);
            }
            None => {
                finish(&mut open, &mut row, shaper);
                cluster.clear();
                cell.text.push_to(&mut cluster);
                let mut one = job();
                one.append(
                    &cluster,
                    0.0,
                    TextFormat {
                        font_id: m.font.clone(),
                        color: fg,
                        italics: italic,
                        ..Default::default()
                    },
                );
                row.runs.push(Run {
                    col,
                    span: if attrs.contains(Attrs::WIDE) { 2 } else { 1 },
                    bold,
                    galley: shaper.fonts.layout_job(one),
                });
            }
        }
    }
    finish(&mut open, &mut row, shaper);
    row
}

/// The egui key as a terminal key, and whether pressing it types text.
fn vt_key(key: egui::Key) -> Option<(Key, bool)> {
    use egui::Key as E;
    let text = |k| Some((k, true));
    let named = |k| Some((k, false));
    match key {
        E::A => text(Key::A),
        E::B => text(Key::B),
        E::C => text(Key::C),
        E::D => text(Key::D),
        E::E => text(Key::E),
        E::F => text(Key::F),
        E::G => text(Key::G),
        E::H => text(Key::H),
        E::I => text(Key::I),
        E::J => text(Key::J),
        E::K => text(Key::K),
        E::L => text(Key::L),
        E::M => text(Key::M),
        E::N => text(Key::N),
        E::O => text(Key::O),
        E::P => text(Key::P),
        E::Q => text(Key::Q),
        E::R => text(Key::R),
        E::S => text(Key::S),
        E::T => text(Key::T),
        E::U => text(Key::U),
        E::V => text(Key::V),
        E::W => text(Key::W),
        E::X => text(Key::X),
        E::Y => text(Key::Y),
        E::Z => text(Key::Z),
        E::Num0 => text(Key::Digit0),
        E::Num1 | E::Exclamationmark => text(Key::Digit1),
        E::Num2 => text(Key::Digit2),
        E::Num3 => text(Key::Digit3),
        E::Num4 => text(Key::Digit4),
        E::Num5 => text(Key::Digit5),
        E::Num6 => text(Key::Digit6),
        E::Num7 => text(Key::Digit7),
        E::Num8 => text(Key::Digit8),
        E::Num9 => text(Key::Digit9),
        E::Backtick => text(Key::Backquote),
        E::Backslash | E::Pipe => text(Key::Backslash),
        E::OpenBracket | E::OpenCurlyBracket => text(Key::BracketLeft),
        E::CloseBracket | E::CloseCurlyBracket => text(Key::BracketRight),
        E::Comma => text(Key::Comma),
        E::Equals | E::Plus => text(Key::Equal),
        E::Minus => text(Key::Minus),
        E::Period => text(Key::Period),
        E::Quote => text(Key::Quote),
        E::Semicolon | E::Colon => text(Key::Semicolon),
        E::Slash | E::Questionmark => text(Key::Slash),
        E::Space => text(Key::Space),
        E::Enter => named(Key::Enter),
        E::Tab => named(Key::Tab),
        E::Escape => named(Key::Escape),
        E::Backspace => named(Key::Backspace),
        E::Delete => named(Key::Delete),
        E::Insert => named(Key::Insert),
        E::Home => named(Key::Home),
        E::End => named(Key::End),
        E::PageUp => named(Key::PageUp),
        E::PageDown => named(Key::PageDown),
        E::ArrowUp => named(Key::ArrowUp),
        E::ArrowDown => named(Key::ArrowDown),
        E::ArrowLeft => named(Key::ArrowLeft),
        E::ArrowRight => named(Key::ArrowRight),
        E::F1 => named(Key::F1),
        E::F2 => named(Key::F2),
        E::F3 => named(Key::F3),
        E::F4 => named(Key::F4),
        E::F5 => named(Key::F5),
        E::F6 => named(Key::F6),
        E::F7 => named(Key::F7),
        E::F8 => named(Key::F8),
        E::F9 => named(Key::F9),
        E::F10 => named(Key::F10),
        E::F11 => named(Key::F11),
        E::F12 => named(Key::F12),
        _ => None,
    }
}

/// What a key types on a US layout with nothing held.
fn unshifted(key: Key) -> Option<char> {
    const LETTERS: [Key; 26] = [
        Key::A,
        Key::B,
        Key::C,
        Key::D,
        Key::E,
        Key::F,
        Key::G,
        Key::H,
        Key::I,
        Key::J,
        Key::K,
        Key::L,
        Key::M,
        Key::N,
        Key::O,
        Key::P,
        Key::Q,
        Key::R,
        Key::S,
        Key::T,
        Key::U,
        Key::V,
        Key::W,
        Key::X,
        Key::Y,
        Key::Z,
    ];
    const DIGITS: [Key; 10] = [
        Key::Digit0,
        Key::Digit1,
        Key::Digit2,
        Key::Digit3,
        Key::Digit4,
        Key::Digit5,
        Key::Digit6,
        Key::Digit7,
        Key::Digit8,
        Key::Digit9,
    ];
    if let Some(i) = LETTERS.iter().position(|k| *k == key) {
        return Some(char::from(b'a' + i as u8));
    }
    if let Some(i) = DIGITS.iter().position(|k| *k == key) {
        return Some(char::from(b'0' + i as u8));
    }
    Some(match key {
        Key::Backquote => '`',
        Key::Backslash => '\\',
        Key::BracketLeft => '[',
        Key::BracketRight => ']',
        Key::Comma => ',',
        Key::Equal => '=',
        Key::Minus => '-',
        Key::Period => '.',
        Key::Quote => '\'',
        Key::Semicolon => ';',
        Key::Slash => '/',
        Key::Space => ' ',
        _ => return None,
    })
}

/// The http(s) URL covering character `col` of `text`.
fn url_in(text: &str, col: usize) -> Option<String> {
    let chars: Vec<char> = text.chars().collect();
    let ends = |c: char| c.is_whitespace() || matches!(c, '"' | '<' | '>' | '`' | '│' | '|');
    if col >= chars.len() || ends(chars[col]) {
        return None;
    }
    let start = (0..=col)
        .rev()
        .find(|i| ends(chars[*i]))
        .map_or(0, |i| i + 1);
    let end = (col..chars.len())
        .find(|i| ends(chars[*i]))
        .unwrap_or(chars.len());
    let word: String = chars[start..end].iter().collect();
    let at = word.find("https://").or_else(|| word.find("http://"))?;
    let url = word[at..].trim_end_matches(['.', ',', ';', ':', ')', ']', '}', '\'', '!', '?']);
    // The pointer must be on the URL, not on what precedes it in the word.
    (start + word[..at].chars().count() <= col && url.len() > "https://".len())
        .then(|| url.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_chords_encode_as_control_bytes() {
        let mut vt = new_vt(80, 24, None).expect("vt");
        let (key, prints) = vt_key(egui::Key::C).expect("mapped");
        assert!(prints);
        let bytes = vt.key(&KeyInput {
            mods: Mods::CTRL,
            unshifted: unshifted(key),
            ..KeyInput::press(key)
        });
        assert_eq!(bytes, [3]);
        assert_eq!(vt.key(&KeyInput::press(Key::Enter)), b"\r");
        assert_eq!(vt.key(&KeyInput::press(Key::ArrowUp)), b"\x1b[A");
    }

    #[test]
    fn typed_text_goes_through_as_typed() {
        let mut vt = new_vt(80, 24, None).expect("vt");
        let bytes = vt.key(&KeyInput {
            mods: Mods::SHIFT,
            consumed: Mods::SHIFT,
            text: Some("A"),
            unshifted: Some('a'),
            ..KeyInput::press(Key::A)
        });
        assert_eq!(bytes, b"A");
    }

    #[test]
    fn finds_the_url_under_the_pointer() {
        let line = "see (https://example.com/a?b=1). and more";
        assert_eq!(
            url_in(line, 10).as_deref(),
            Some("https://example.com/a?b=1")
        );
        assert_eq!(url_in(line, 4), None);
        assert_eq!(url_in(line, 1), None);
        assert_eq!(url_in(line, 36), None);
        assert_eq!(url_in("", 0), None);
    }

    #[test]
    fn every_printing_key_has_its_unshifted_character() {
        for key in [egui::Key::A, egui::Key::Num7, egui::Key::Slash] {
            let (k, prints) = vt_key(key).expect("mapped");
            assert!(prints);
            assert!(unshifted(k).is_some());
        }
        assert_eq!(vt_key(egui::Key::Enter).map(|k| k.1), Some(false));
    }
}
