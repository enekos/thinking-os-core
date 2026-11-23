//! [`Vt`]: one terminal.

#![allow(
    unsafe_code,
    reason = "owns the libghostty-vt handles and hosts the callbacks it calls"
)]

use std::ffi::c_void;
use std::fmt;
use std::ptr::{self, NonNull};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::ffi::{self, Term};
use crate::input::{
    Key, KeyAction, KeyInput, Mods, MouseAction, MouseButton, MouseInput, Scroll, SearchMatch,
    SearchStatus, SelectPoint, SelectStep,
};
use crate::sys;
use crate::types::{
    Attrs, Cell, Colors, Cursor, CursorStyle, Error, Event, Frame, Grapheme, Line, Options,
    ProgramStatus, Rgb, Scrollbar, StatusKind, StatusState, Underline,
};

/// The largest clipboard write (OSC 52) a program may make.
const CLIPBOARD_MAX_BYTES: usize = 1024 * 1024;
/// Replies nobody collected stop accumulating here.
const REPLIES_MAX_BYTES: usize = 1024 * 1024;
/// Events nobody collected stop accumulating here.
const EVENTS_MAX: usize = 4096;
/// How long a program may hold rendering (mode 2026) before it is overruled.
const HOLD_TIMEOUT: Duration = Duration::from_secs(1);
/// Presses closer together than this are a double or triple click.
const MULTI_CLICK_NS: u64 = 500_000_000;
/// The selection gesture takes whole pixels, cells can be fractional: feed it
/// 1/64 pixel units instead.
const SUBPIXEL: f64 = 64.0;
/// Kitty's Unicode placeholder for images; never text.
const KITTY_PLACEHOLDER: char = '\u{10EEEE}';

const MODE_FOCUS_EVENTS: u16 = 1004;
const MODE_MOUSE_SGR_PIXELS: u16 = 1016;
const MODE_BRACKETED_PASTE: u16 = 2004;
const MODE_SYNCHRONIZED_OUTPUT: u16 = 2026;
const MODE_GRAPHEME_CLUSTER: u16 = 2027;

/// Puts a fresh terminal of the same size back to a known state before a
/// repaint: soft reset, every mouse/paste/focus/sync mode off, legacy
/// keyboard, primary screen, cleared.
const REPAINT_RESET: &[u8] =
    b"\x1b[!p\x1b[?2026l\x1b[?9l\x1b[?1000l\x1b[?1001l\x1b[?1002l\x1b[?1003l\
\x1b[?1004l\x1b[?1005l\x1b[?1006l\x1b[?1015l\x1b[?1016l\x1b[?2004l\x1b[?1l\x1b[?66l\x1b[?2031l\
\x1b[?2048l\x1b[=0;1u\x1b[?1049l\x1b[?1047l\x1b[?47l\x1b[H\x1b[2J";
/// Mode sets a repaint must not replay: they would hold the viewer's
/// rendering or switch (and clear) the screen a second time.
const REPAINT_STRIP: [&[u8]; 4] = [b"\x1b[?2026h", b"\x1b[?47h", b"\x1b[?1047h", b"\x1b[?1049h"];

/// What the callbacks write and [`Vt`] reads. Lives in a `Box` so its address
/// survives moves of the `Vt`; libghostty-vt holds that address as userdata.
struct Shared {
    replies: Vec<u8>,
    events: Vec<Event>,
    title_changed: bool,
    pwd_changed: bool,
    /// The render state, so a hold can capture the frame it begins on.
    render: sys::GhosttyRenderState,
    held: bool,
    hold_started: Option<Instant>,
    cols: u16,
    rows: u16,
    cell_w: u32,
    cell_h: u32,
}

impl Shared {
    fn push(&mut self, event: Event) {
        let repeated_bell = event == Event::Bell && self.events.last() == Some(&Event::Bell);
        if !repeated_bell && self.events.len() < EVENTS_MAX {
            self.events.push(event);
        }
    }
}

/// The `Shared` behind a callback's userdata.
///
/// # Safety
///
/// `userdata` must be NULL or the pointer [`Vt::new`] registered, and no other
/// reference to that `Shared` may be live.
unsafe fn shared_from<'a>(userdata: *mut c_void) -> Option<&'a mut Shared> {
    // SAFETY: per this function's contract the pointer is NULL or a live,
    // unaliased `Shared`.
    unsafe { userdata.cast::<Shared>().as_mut() }
}

// The callbacks. libghostty-vt calls them synchronously from inside
// `ghostty_terminal_vt_write`, `_resize` and `_set`, on the calling thread,
// with the userdata `Vt::new` registered. `Vt` never holds a reference to its
// `Shared` across one of those calls, so the exclusive borrow each callback
// takes is the only one. They touch nothing but `Shared` and never unwind:
// the only way out other than returning is an allocation failure, which
// aborts.

/// Replies the terminal wants written to the program (DA, DSR, ...).
///
/// # Safety
///
/// Called by libghostty-vt only; see the note above.
unsafe extern "C" fn on_write_pty(
    _terminal: sys::GhosttyTerminal,
    userdata: *mut c_void,
    data: *const u8,
    len: usize,
) {
    // SAFETY: the library passes back the registered userdata; see above.
    let Some(shared) = (unsafe { shared_from(userdata) }) else {
        return;
    };
    if data.is_null() || len == 0 || shared.replies.len() + len > REPLIES_MAX_BYTES {
        return;
    }
    // SAFETY: the library passes `len` readable bytes that live for the call.
    let bytes = unsafe { std::slice::from_raw_parts(data, len) };
    shared.replies.extend_from_slice(bytes);
}

/// BEL.
///
/// # Safety
///
/// Called by libghostty-vt only; see the note above.
unsafe extern "C" fn on_bell(_terminal: sys::GhosttyTerminal, userdata: *mut c_void) {
    // SAFETY: the library passes back the registered userdata; see above.
    if let Some(shared) = unsafe { shared_from(userdata) } {
        shared.push(Event::Bell);
    }
}

/// OSC 0/2. The title is read after the write returns.
///
/// # Safety
///
/// Called by libghostty-vt only; see the note above.
unsafe extern "C" fn on_title_changed(_terminal: sys::GhosttyTerminal, userdata: *mut c_void) {
    // SAFETY: the library passes back the registered userdata; see above.
    if let Some(shared) = unsafe { shared_from(userdata) } {
        shared.title_changed = true;
    }
}

/// OSC 7. The directory is read after the write returns.
///
/// # Safety
///
/// Called by libghostty-vt only; see the note above.
unsafe extern "C" fn on_pwd_changed(_terminal: sys::GhosttyTerminal, userdata: *mut c_void) {
    // SAFETY: the library passes back the registered userdata; see above.
    if let Some(shared) = unsafe { shared_from(userdata) } {
        shared.pwd_changed = true;
    }
}

/// Size queries (XTWINOPS, mode 2048 reports).
///
/// # Safety
///
/// Called by libghostty-vt only; see the note above.
unsafe extern "C" fn on_size(
    _terminal: sys::GhosttyTerminal,
    userdata: *mut c_void,
    out_size: *mut sys::GhosttySizeReportSize,
) -> bool {
    // SAFETY: the library passes back the registered userdata; see above.
    let Some(shared) = (unsafe { shared_from(userdata) }) else {
        return false;
    };
    // SAFETY: the library passes NULL or a writable size struct.
    let Some(out) = (unsafe { out_size.as_mut() }) else {
        return false;
    };
    if shared.cell_w == 0 || shared.cell_h == 0 {
        return false;
    }
    out.rows = shared.rows;
    out.columns = shared.cols;
    out.cell_width = shared.cell_w;
    out.cell_height = shared.cell_h;
    true
}

/// Device attributes: a VT220-class terminal with ANSI color.
///
/// # Safety
///
/// Called by libghostty-vt only; see the note above.
unsafe extern "C" fn on_device_attributes(
    _terminal: sys::GhosttyTerminal,
    _userdata: *mut c_void,
    out_attrs: *mut sys::GhosttyDeviceAttributes,
) -> bool {
    // SAFETY: the library passes NULL or a writable attributes struct.
    let Some(out) = (unsafe { out_attrs.as_mut() }) else {
        return false;
    };
    out.primary.conformance_level = sys::GHOSTTY_DA_CONFORMANCE_LEVEL_2 as u16;
    out.primary.features[0] = sys::GHOSTTY_DA_FEATURE_ANSI_COLOR as u16;
    out.primary.num_features = 1;
    out.secondary.device_type = sys::GHOSTTY_DA_DEVICE_TYPE_VT220 as u16;
    out.secondary.firmware_version = 10;
    out.secondary.rom_cartridge = 0;
    out.tertiary.unit_id = 0;
    true
}

/// A clipboard write (OSC 52): text up to 1 MiB becomes an event.
///
/// # Safety
///
/// Called by libghostty-vt only; see the note above.
unsafe extern "C" fn on_clipboard_write(
    _terminal: sys::GhosttyTerminal,
    userdata: *mut c_void,
    write: *const sys::GhosttyClipboardWrite,
) {
    // SAFETY: the library passes NULL or a request that lives for the call.
    let Some(request) = (unsafe { write.as_ref() }) else {
        return;
    };
    let contents: &[sys::GhosttyClipboardContent] =
        if request.contents.is_null() || request.contents_len == 0 {
            &[]
        } else {
            // SAFETY: the request describes `contents_len` readable entries
            // that live for the call.
            unsafe { std::slice::from_raw_parts(request.contents, request.contents_len) }
        };
    let text = match contents.first() {
        None => Some(String::new()),
        Some(_) => contents
            .iter()
            .find(|content| {
                // SAFETY: the library lends each entry's mime string for the
                // call.
                let mime = unsafe { ffi::lent_string(content.mime) };
                mime.is_empty() || mime.starts_with("text/plain")
            })
            .filter(|content| content.data.len <= CLIPBOARD_MAX_BYTES)
            // SAFETY: the library lends each entry's data for the call.
            .map(|content| unsafe { ffi::lent_string(content.data) }),
    };
    let result = if text.is_some() {
        sys::GHOSTTY_CLIPBOARD_WRITE_RESULT_SUCCESS
    } else {
        sys::GHOSTTY_CLIPBOARD_WRITE_RESULT_DENIED
    };
    // SAFETY: the library passes back the registered userdata; see above.
    if let (Some(shared), Some(text)) = (unsafe { shared_from(userdata) }, text) {
        shared.push(Event::Clipboard(text));
    }
    if let Some(reply) = request.reply {
        let answer = sys::GhosttyClipboardWriteReply {
            size: size_of::<sys::GhosttyClipboardWriteReply>(),
            result,
            remember: false,
        };
        // SAFETY: `reply` is the function the library put in the request, and
        // it takes that same request plus a sized reply, both live here.
        unsafe { reply(write, &raw const answer) };
    }
}

/// A program status report (OSC 7501).
///
/// # Safety
///
/// Called by libghostty-vt only; see the note above.
unsafe extern "C" fn on_program_status(
    _terminal: sys::GhosttyTerminal,
    userdata: *mut c_void,
    report: *const sys::GhosttyTerminalProgramStatus,
) {
    // SAFETY: the library passes back the registered userdata; see above.
    let Some(shared) = (unsafe { shared_from(userdata) }) else {
        return;
    };
    // SAFETY: the library passes NULL or a report that lives for the call.
    let Some(report) = (unsafe { report.as_ref() }) else {
        return;
    };
    let state = match report.state {
        sys::GHOSTTY_PROGRAM_STATUS_STATE_WORKING => StatusState::Working,
        sys::GHOSTTY_PROGRAM_STATUS_STATE_DONE => StatusState::Done,
        sys::GHOSTTY_PROGRAM_STATUS_STATE_BLOCKED => StatusState::Blocked,
        sys::GHOSTTY_PROGRAM_STATUS_STATE_ERROR => StatusState::Error,
        sys::GHOSTTY_PROGRAM_STATUS_STATE_CLEAR => StatusState::Clear,
        _ => StatusState::Idle,
    };
    let kind = match report.kind {
        sys::GHOSTTY_PROGRAM_STATUS_KIND_PERMISSION => StatusKind::Permission,
        sys::GHOSTTY_PROGRAM_STATUS_KIND_QUESTION => StatusKind::Question,
        sys::GHOSTTY_PROGRAM_STATUS_KIND_AUTH => StatusKind::Auth,
        _ => StatusKind::None,
    };
    // SAFETY: (all four) the library lends the report's strings for the call.
    let id = unsafe { ffi::lent_string(report.id) };
    // SAFETY: as above.
    let app = unsafe { ffi::lent_string(report.app) };
    // SAFETY: as above.
    let title = unsafe { ffi::lent_string(report.title) };
    // SAFETY: as above.
    let message = unsafe { ffi::lent_string(report.message) };
    shared.push(Event::Status(ProgramStatus {
        state,
        kind,
        progress: u8::try_from(report.progress).ok(),
        id,
        app,
        title,
        message,
    }));
}

/// Synchronized output (mode 2026) began or ended.
///
/// # Safety
///
/// Called by libghostty-vt only; see the note above.
unsafe extern "C" fn on_render_hold(
    terminal: sys::GhosttyTerminal,
    userdata: *mut c_void,
    held: bool,
) {
    // SAFETY: the library passes back the registered userdata; see above.
    let Some(shared) = (unsafe { shared_from(userdata) }) else {
        return;
    };
    if !held {
        shared.held = false;
        shared.hold_started = None;
        return;
    }
    if shared.held {
        return;
    }
    if !shared.render.is_null() {
        // Capture the frame the hold begins on: that, not whatever was drawn
        // last, is what stays on screen until the program releases the hold.
        // The header's own example does this from inside the callback.
        ffi::render_update(shared.render, Term(terminal));
    }
    shared.held = true;
    shared.hold_started = Some(Instant::now());
}

// The callbacks are handed over as `void*`; these fail to compile if one
// drifts from the type the header declares.
const _: sys::GhosttyTerminalWritePtyFn = Some(on_write_pty);
const _: sys::GhosttyTerminalBellFn = Some(on_bell);
const _: sys::GhosttyTerminalTitleChangedFn = Some(on_title_changed);
const _: sys::GhosttyTerminalPwdChangedFn = Some(on_pwd_changed);
const _: sys::GhosttyTerminalSizeFn = Some(on_size);
const _: sys::GhosttyTerminalDeviceAttributesFn = Some(on_device_attributes);
const _: sys::GhosttyTerminalClipboardWriteFn = Some(on_clipboard_write);
const _: sys::GhosttyTerminalProgramStatusFn = Some(on_program_status);
const _: sys::GhosttyTerminalRenderHoldFn = Some(on_render_hold);

/// A row of the previous frame.
struct CachedRow {
    id: [u64; 2],
    selection: Option<(u16, u16)>,
    line: Arc<Line>,
}

/// Everything in a frame besides the rows; what `changed` compares.
#[derive(Clone, Copy, PartialEq, Eq)]
struct FrameKey {
    cols: u16,
    rows: u16,
    cursor: Option<Cursor>,
    foreground: Rgb,
    background: Rgb,
    scrollbar: Scrollbar,
    at_bottom: bool,
}

/// The render state's non-row data, read once per `changed`/`frame`.
struct View {
    key: FrameKey,
    palette: [sys::GhosttyColorRgb; 256],
}

/// One terminal: Ghostty's emulator state plus what a host needs around it.
///
/// Feed it the program's output with [`Vt::write`], collect what must go back
/// to the program with [`Vt::take_replies`] and the input encoders
/// ([`Vt::key`], [`Vt::mouse`], [`Vt::paste`], [`Vt::focus`]), and draw
/// [`Vt::frame`] whenever [`Vt::changed`] says so.
///
/// `Vt` is `Send` but not `Sync`: move it between threads freely, share it
/// behind a `Mutex`.
pub struct Vt {
    term: sys::GhosttyTerminal,
    render: sys::GhosttyRenderState,
    row_iter: sys::GhosttyRenderStateRowIterator,
    row_cells: sys::GhosttyRenderStateRowCells,
    key_encoder: sys::GhosttyKeyEncoder,
    key_event: sys::GhosttyKeyEvent,
    mouse_encoder: sys::GhosttyMouseEncoder,
    mouse_event: sys::GhosttyMouseEvent,
    gesture: sys::GhosttySelectionGesture,
    press_event: sys::GhosttySelectionGestureEvent,
    drag_event: sys::GhosttySelectionGestureEvent,
    release_event: sys::GhosttySelectionGestureEvent,
    search: sys::GhosttySearch,
    /// Owned; freed in `Drop` after the terminal that points at it.
    shared: NonNull<Shared>,

    selection_colors: Option<(Rgb, Option<Rgb>)>,
    rows: Vec<CachedRow>,
    palette: [sys::GhosttyColorRgb; 256],
    last_key: Option<FrameKey>,
    /// Rebuild every row on the next frame (theme or size changed).
    force: bool,
    /// The next frame differs in a way the dirty flags may not show
    /// (selection, scroll, search).
    stale: bool,
    /// The user did something that should show at once: the next refresh
    /// ignores a render hold.
    live: bool,
    hold_timeout: Duration,
    search_active: bool,
    /// Whether the search has seen the terminal since its last mutation.
    /// Match refs are only valid while this is true.
    search_fresh: bool,
    last_mouse_cell: Option<(u16, u16)>,
    buttons_down: u8,
    epoch: Instant,
    scratch_cells: Vec<Cell>,
    scratch_codepoints: Vec<u32>,
}

// SAFETY: every handle and the `Shared` box are owned by this value alone and
// reachable only through it. libghostty-vt keeps no thread-local or
// thread-affine state: its header requires only that calls on one terminal
// are not concurrent, and says callbacks run on whichever thread calls
// `ghostty_terminal_vt_write`. `&mut self`/`&self` plus the absence of `Sync`
// (raw pointers) guarantee exactly that serialization. `Shared` holds only
// `Send` data and one of those same handles.
unsafe impl Send for Vt {}

impl fmt::Debug for Vt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (cols, rows) = self.size();
        f.debug_struct("Vt")
            .field("cols", &cols)
            .field("rows", &rows)
            .field("title", &self.title())
            .field("alt_screen", &self.alt_screen())
            .finish_non_exhaustive()
    }
}

fn check(result: sys::GhosttyResult) -> Result<(), Error> {
    if ffi::ok(result) {
        Ok(())
    } else if result == sys::GHOSTTY_OUT_OF_MEMORY {
        Err(Error::OutOfMemory)
    } else {
        Err(Error::Ghostty(result))
    }
}

impl Vt {
    /// Creates a terminal.
    ///
    /// # Errors
    ///
    /// When libghostty-vt cannot allocate one of its objects.
    pub fn new(options: &Options) -> Result<Self, Error> {
        let cols = options.cols.max(1);
        let rows = options.rows.max(1);
        let shared = Box::new(Shared {
            replies: Vec::new(),
            events: Vec::new(),
            title_changed: false,
            pwd_changed: false,
            render: ptr::null_mut(),
            held: false,
            hold_started: None,
            cols,
            rows,
            cell_w: 0,
            cell_h: 0,
        });
        // Every handle starts NULL, and `Drop` skips NULL handles, so an
        // early return from `init` frees exactly what was created.
        let mut vt = Self {
            term: ptr::null_mut(),
            render: ptr::null_mut(),
            row_iter: ptr::null_mut(),
            row_cells: ptr::null_mut(),
            key_encoder: ptr::null_mut(),
            key_event: ptr::null_mut(),
            mouse_encoder: ptr::null_mut(),
            mouse_event: ptr::null_mut(),
            gesture: ptr::null_mut(),
            press_event: ptr::null_mut(),
            drag_event: ptr::null_mut(),
            release_event: ptr::null_mut(),
            search: ptr::null_mut(),
            shared: NonNull::from(Box::leak(shared)),
            selection_colors: None,
            rows: Vec::new(),
            palette: [sys::GhosttyColorRgb::default(); 256],
            last_key: None,
            force: true,
            stale: false,
            live: false,
            hold_timeout: HOLD_TIMEOUT,
            search_active: false,
            search_fresh: false,
            last_mouse_cell: None,
            buttons_down: 0,
            epoch: Instant::now(),
            scratch_cells: Vec::new(),
            scratch_codepoints: Vec::new(),
        };
        vt.init(options, cols, rows)?;
        Ok(vt)
    }

    fn init(&mut self, options: &Options, cols: u16, rows: u16) -> Result<(), Error> {
        // SAFETY: (every `_new` below) the out-pointer is a writable handle
        // slot, and NULL selects the default allocator.
        check(unsafe { sys::ghostty_terminal_new(ptr::null(), &raw mut self.term, cols, rows) })?;
        // SAFETY: as above.
        check(unsafe { sys::ghostty_render_state_new(ptr::null(), &raw mut self.render) })?;
        // SAFETY: as above.
        check(unsafe {
            sys::ghostty_render_state_row_iterator_new(ptr::null(), &raw mut self.row_iter)
        })?;
        // SAFETY: as above.
        check(unsafe {
            sys::ghostty_render_state_row_cells_new(ptr::null(), &raw mut self.row_cells)
        })?;
        // SAFETY: as above.
        check(unsafe { sys::ghostty_key_encoder_new(ptr::null(), &raw mut self.key_encoder) })?;
        // SAFETY: as above.
        check(unsafe { sys::ghostty_key_event_new(ptr::null(), &raw mut self.key_event) })?;
        // SAFETY: as above.
        check(unsafe { sys::ghostty_mouse_encoder_new(ptr::null(), &raw mut self.mouse_encoder) })?;
        // SAFETY: as above.
        check(unsafe { sys::ghostty_mouse_event_new(ptr::null(), &raw mut self.mouse_event) })?;
        // SAFETY: as above.
        check(unsafe { sys::ghostty_selection_gesture_new(ptr::null(), &raw mut self.gesture) })?;
        // SAFETY: as above.
        check(unsafe {
            sys::ghostty_selection_gesture_event_new(
                ptr::null(),
                &raw mut self.press_event,
                sys::GHOSTTY_SELECTION_GESTURE_EVENT_TYPE_PRESS,
            )
        })?;
        // SAFETY: as above.
        check(unsafe {
            sys::ghostty_selection_gesture_event_new(
                ptr::null(),
                &raw mut self.drag_event,
                sys::GHOSTTY_SELECTION_GESTURE_EVENT_TYPE_DRAG,
            )
        })?;
        // SAFETY: as above.
        check(unsafe {
            sys::ghostty_selection_gesture_event_new(
                ptr::null(),
                &raw mut self.release_event,
                sys::GHOSTTY_SELECTION_GESTURE_EVENT_TYPE_RELEASE,
            )
        })?;
        // SAFETY: as above; the terminal was created a few lines up and
        // outlives the search (see `Drop`).
        check(unsafe { sys::ghostty_search_new(ptr::null(), &raw mut self.search, self.term) })?;

        self.shared().render = self.render;
        let term = self.term();
        let userdata = self.shared.as_ptr().cast::<c_void>().cast_const();
        let callbacks = [
            (sys::GHOSTTY_TERMINAL_OPT_USERDATA, userdata),
            (
                sys::GHOSTTY_TERMINAL_OPT_WRITE_PTY,
                on_write_pty as *const c_void,
            ),
            (sys::GHOSTTY_TERMINAL_OPT_BELL, on_bell as *const c_void),
            (
                sys::GHOSTTY_TERMINAL_OPT_TITLE_CHANGED,
                on_title_changed as *const c_void,
            ),
            (
                sys::GHOSTTY_TERMINAL_OPT_PWD_CHANGED,
                on_pwd_changed as *const c_void,
            ),
            (sys::GHOSTTY_TERMINAL_OPT_SIZE, on_size as *const c_void),
            (
                sys::GHOSTTY_TERMINAL_OPT_DEVICE_ATTRIBUTES,
                on_device_attributes as *const c_void,
            ),
            (
                sys::GHOSTTY_TERMINAL_OPT_CLIPBOARD_WRITE,
                on_clipboard_write as *const c_void,
            ),
            (
                sys::GHOSTTY_TERMINAL_OPT_PROGRAM_STATUS,
                on_program_status as *const c_void,
            ),
            (
                sys::GHOSTTY_TERMINAL_OPT_RENDER_HOLD,
                on_render_hold as *const c_void,
            ),
        ];
        for (option, value) in callbacks {
            if !term.set_pointer(option, value) {
                return Err(Error::Ghostty(sys::GHOSTTY_INVALID_VALUE));
            }
        }

        // Grapheme clustering on from the start (and after a reset), so an
        // emoji with a variation selector or a ZWJ sequence is one cell.
        let grapheme = sys::GhosttyTerminalModeConfig {
            mode: ffi::dec_mode(MODE_GRAPHEME_CLUSTER),
            value: true,
        };
        term.set(ffi::O_MODE_DEFAULT, Some(&grapheme));
        term.set(ffi::O_MODE, Some(&grapheme));
        term.set(ffi::O_SCROLLBACK_MAX_BYTES, Some(&options.scrollback_bytes));
        term.set(ffi::O_CLIPBOARD_WRITE_MAX_BYTES, Some(&CLIPBOARD_MAX_BYTES));
        // Kitty graphics are not exposed by this crate: a zero budget turns
        // the protocol off, so image sequences are parsed and dropped.
        term.set(ffi::O_KITTY_IMAGE_STORAGE_LIMIT, Some(&0u64));
        if let Some(colors) = &options.colors {
            self.set_colors(colors);
        }
        Ok(())
    }

    fn term(&self) -> Term {
        Term(self.term)
    }

    /// The callback state. The borrow must end before the next libghostty-vt
    /// call that can run a callback; every use here is a single statement.
    fn shared(&mut self) -> &mut Shared {
        // SAFETY: the box is live until `Drop`, and callbacks (the only other
        // users) run only inside FFI calls, none of which is in progress
        // while safe code holds this borrow.
        unsafe { self.shared.as_mut() }
    }

    fn shared_ref(&self) -> &Shared {
        // SAFETY: as in `shared`.
        unsafe { self.shared.as_ref() }
    }

    /// Feeds the program's output to the terminal.
    pub fn write(&mut self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        self.term().write(bytes);
        self.search_fresh = false;
        self.collect_changes();
    }

    /// Turns the "title/pwd changed" marks the callbacks left into events
    /// carrying the current values.
    fn collect_changes(&mut self) {
        if std::mem::take(&mut self.shared().title_changed) {
            let title = self.title().unwrap_or_default();
            self.shared().push(Event::Title(title));
        }
        if std::mem::take(&mut self.shared().pwd_changed) {
            let pwd = self.pwd().unwrap_or_default();
            self.shared().push(Event::Pwd(pwd));
        }
    }

    /// Resizes the grid, reflowing soft-wrapped lines. The cell size in
    /// pixels is only reported to programs that ask (XTWINOPS, mode 2048);
    /// pass zero when unknown. May queue a reply (in-band size reports).
    pub fn resize(&mut self, cols: u16, rows: u16, cell_w_px: u32, cell_h_px: u32) {
        let cols = cols.max(1);
        let rows = rows.max(1);
        let shared = self.shared();
        shared.cols = cols;
        shared.rows = rows;
        shared.cell_w = cell_w_px;
        shared.cell_h = cell_h_px;
        self.term().resize(cols, rows, cell_w_px, cell_h_px);
        self.force = true;
        self.live = true;
        self.search_fresh = false;
        self.last_mouse_cell = None;
    }

    /// `(cols, rows)`.
    #[must_use]
    pub fn size(&self) -> (u16, u16) {
        self.term().size()
    }

    /// Applies a theme. Colors a program set itself (OSC 4/10/11/12) are
    /// overwritten.
    pub fn set_colors(&mut self, colors: &Colors) {
        let term = self.term();
        term.set(ffi::O_FOREGROUND, Some(&ffi::raw_rgb(colors.foreground)));
        term.set(ffi::O_BACKGROUND, Some(&ffi::raw_rgb(colors.background)));
        term.set(ffi::O_CURSOR, colors.cursor.map(ffi::raw_rgb).as_ref());
        let mut palette = [sys::GhosttyColorRgb::default(); 256];
        // SAFETY: the function writes exactly 256 entries, the array's size.
        unsafe { sys::ghostty_color_palette_default(palette.as_mut_ptr()) };
        for (slot, color) in palette.iter_mut().zip(colors.ansi) {
            *slot = ffi::raw_rgb(color);
        }
        term.set(ffi::O_PALETTE, Some(&palette));
        self.selection_colors = colors.selection;
        self.force = true;
        self.live = true;
    }

    /// Takes the bytes the terminal wants written to the program: answers to
    /// queries (DA, DSR, size reports...). Check after every [`Vt::write`]
    /// and [`Vt::resize`].
    pub fn take_replies(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.shared().replies)
    }

    /// Takes the events since the last call, oldest first. Title and
    /// directory changes come last within one [`Vt::write`] and carry the
    /// final value; repeated bells collapse into one.
    pub fn take_events(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.shared().events)
    }

    /// The title the program set, if any.
    #[must_use]
    pub fn title(&self) -> Option<String> {
        self.term().string(ffi::T_TITLE)
    }

    /// The working directory the program reported (OSC 7), as sent.
    #[must_use]
    pub fn pwd(&self) -> Option<String> {
        self.term().string(ffi::T_PWD)
    }

    /// Whether DEC private mode `mode` (the `n` of `CSI ? n h`) is set.
    #[must_use]
    pub fn mode(&self, mode: u16) -> bool {
        self.term().mode(ffi::dec_mode(mode))
    }

    /// Whether the alternate screen is showing.
    #[must_use]
    pub fn alt_screen(&self) -> bool {
        self.term().alt_screen()
    }

    /// Whether the program asked for mouse reports (any tracking mode).
    #[must_use]
    pub fn mouse_tracking(&self) -> bool {
        self.term().get(ffi::T_MOUSE_TRACKING).unwrap_or(false)
    }

    /// Whether bracketed paste (mode 2004) is on.
    #[must_use]
    pub fn bracketed_paste(&self) -> bool {
        self.mode(MODE_BRACKETED_PASTE)
    }

    /// The kitty keyboard protocol flags the program enabled; 0 is legacy
    /// encoding.
    #[must_use]
    pub fn kitty_keyboard_flags(&self) -> u8 {
        self.term().get(ffi::T_KITTY_FLAGS).unwrap_or(0)
    }

    /// Whether the program is holding rendering (synchronized output, mode
    /// 2026). While it does, [`Vt::frame`] keeps returning the screen as it
    /// was when the hold began.
    #[must_use]
    pub fn render_held(&self) -> bool {
        self.shared_ref().held
    }

    /// How long a program may hold rendering before the hold is broken.
    /// One second by default.
    pub fn set_hold_timeout(&mut self, timeout: Duration) {
        self.hold_timeout = timeout;
    }

    /// Ends an overdue hold, then brings the render state up to date unless
    /// a hold is in force.
    fn refresh(&mut self) {
        let overdue = self
            .shared_ref()
            .hold_started
            .is_some_and(|started| started.elapsed() >= self.hold_timeout);
        if overdue {
            // The terminal has no clock; resetting the mode is how a host
            // ends a hold. This runs `on_render_hold(false)`.
            let off = sys::GhosttyTerminalModeConfig {
                mode: ffi::dec_mode(MODE_SYNCHRONIZED_OUTPUT),
                value: false,
            };
            self.term().set(ffi::O_MODE, Some(&off));
            let shared = self.shared();
            shared.held = false;
            shared.hold_started = None;
        }
        if self.live || !self.shared_ref().held {
            ffi::render_update(self.render, self.term());
        }
    }

    fn view(&self) -> View {
        let mut colors = sys::GhosttyRenderStateColors {
            size: size_of::<sys::GhosttyRenderStateColors>(),
            ..Default::default()
        };
        ffi::render_get(self.render, ffi::R_COLORS, &mut colors);
        let mut raw = sys::GhosttyRenderStateCursor {
            size: size_of::<sys::GhosttyRenderStateCursor>(),
            ..Default::default()
        };
        ffi::render_get(self.render, ffi::R_CURSOR, &mut raw);
        let mut cols = 0u16;
        ffi::render_get(self.render, ffi::R_COLS, &mut cols);
        let mut rows = 0u16;
        ffi::render_get(self.render, ffi::R_ROWS, &mut rows);

        let cursor = (raw.visible && raw.viewport_has_value && raw.viewport_y < rows).then(|| {
            let style = match raw.visual_style {
                sys::GHOSTTY_RENDER_STATE_CURSOR_VISUAL_STYLE_BAR => CursorStyle::Bar,
                sys::GHOSTTY_RENDER_STATE_CURSOR_VISUAL_STYLE_UNDERLINE => CursorStyle::Underline,
                sys::GHOSTTY_RENDER_STATE_CURSOR_VISUAL_STYLE_BLOCK_HOLLOW => {
                    CursorStyle::HollowBlock
                }
                _ => CursorStyle::Block,
            };
            Cursor {
                x: raw.viewport_x.saturating_sub(u16::from(raw.wide_tail)),
                y: raw.viewport_y,
                style,
                blinking: raw.blinking,
                color: colors.cursor_has_value.then(|| ffi::rgb(colors.cursor)),
            }
        });

        // The scroll position comes from the terminal, not the render state;
        // while a hold freezes the latter, keep the two consistent.
        let frozen = self
            .last_key
            .filter(|_| self.shared_ref().held && !self.live);
        let (scrollbar, at_bottom) = match frozen {
            Some(key) => (key.scrollbar, key.at_bottom),
            None => {
                let raw = self.term().get(ffi::T_SCROLLBAR).unwrap_or_default();
                (
                    Scrollbar {
                        total: raw.total,
                        offset: raw.offset,
                        len: raw.len,
                    },
                    self.term().get(ffi::T_VIEWPORT_ACTIVE).unwrap_or(true),
                )
            }
        };

        View {
            key: FrameKey {
                cols,
                rows,
                cursor,
                foreground: ffi::rgb(colors.foreground),
                background: ffi::rgb(colors.background),
                scrollbar,
                at_bottom,
            },
            palette: colors.palette,
        }
    }

    /// Whether [`Vt::frame`] would return something different from the last
    /// frame: a row, the cursor, the colors, the selection or the scroll
    /// position changed. Cheap; call it every tick.
    pub fn changed(&mut self) -> bool {
        self.refresh();
        if self.force || self.stale || self.last_key.is_none() {
            return true;
        }
        let mut dirty = sys::GHOSTTY_RENDER_STATE_DIRTY_FALSE;
        ffi::render_get(self.render, ffi::R_DIRTY, &mut dirty);
        dirty != sys::GHOSTTY_RENDER_STATE_DIRTY_FALSE || Some(self.view().key) != self.last_key
    }

    /// The viewport, ready to draw.
    ///
    /// Rows that did not change since the previous call are the same `Arc`.
    /// While the program holds rendering (synchronized output), this keeps
    /// returning the screen as it was when the hold began; a hold older than
    /// the timeout (one second) is broken. Scrolling, selecting, resizing and
    /// theme changes show at once regardless.
    pub fn frame(&mut self) -> Frame {
        self.refresh();
        let view = self.view();
        let theme_changed = self.last_key.is_none_or(|last| {
            last.foreground != view.key.foreground || last.background != view.key.background
        }) || !same_palette(&self.palette, &view.palette);
        if theme_changed {
            self.palette = view.palette;
        }
        let mut dirty = sys::GHOSTTY_RENDER_STATE_DIRTY_FULL;
        ffi::render_get(self.render, ffi::R_DIRTY, &mut dirty);
        let rebuild_all =
            self.force || theme_changed || dirty == sys::GHOSTTY_RENDER_STATE_DIRTY_FULL;
        let lines = self.build_rows(&view, rebuild_all);
        ffi::render_clean(self.render);
        self.last_key = Some(view.key);
        self.force = false;
        self.stale = false;
        self.live = false;
        Frame {
            cols: view.key.cols,
            rows: view.key.rows,
            lines,
            cursor: view.key.cursor,
            foreground: view.key.foreground,
            background: view.key.background,
            scrollbar: view.key.scrollbar,
            at_bottom: view.key.at_bottom,
        }
    }

    fn build_rows(&mut self, view: &View, rebuild_all: bool) -> Vec<Arc<Line>> {
        let cols = usize::from(view.key.cols);
        let rows = usize::from(view.key.rows);
        let old = std::mem::take(&mut self.rows);
        let mut cache = Vec::with_capacity(rows);
        let mut lines = Vec::with_capacity(rows);
        if ffi::render_get(self.render, ffi::R_ROW_ITERATOR, &mut self.row_iter) {
            while lines.len() < rows && ffi::row_next(self.row_iter) {
                let y = lines.len();
                let mut row_dirty = true;
                if !ffi::row_get(self.row_iter, ffi::ROW_DIRTY, &mut row_dirty) {
                    row_dirty = true;
                }
                let mut id = sys::GhosttyRenderStateRowId::default();
                let has_id = ffi::row_get(self.row_iter, ffi::ROW_ID, &mut id);
                let mut range = sys::GhosttyRenderStateRowSelection {
                    size: size_of::<sys::GhosttyRenderStateRowSelection>(),
                    ..Default::default()
                };
                let selection = ffi::row_get(self.row_iter, ffi::ROW_SELECTION, &mut range)
                    .then_some((range.start_x, range.end_x));

                let same_place = old.get(y).filter(|row| has_id && row.id == id.bits);
                let untouched = same_place.filter(|row| {
                    !rebuild_all
                        && !row_dirty
                        && row.selection == selection
                        && row.line.cells.len() == cols
                });
                let line = if let Some(row) = untouched {
                    Arc::clone(&row.line)
                } else {
                    let wrapped = self.read_row(view, selection);
                    // A rebuilt row that turns out identical keeps its `Arc`,
                    // wherever scrolling moved it.
                    let previous = same_place.or_else(|| {
                        old.iter()
                            .filter(|_| has_id)
                            .find(|row| row.id == id.bits)
                            .or_else(|| old.get(y))
                    });
                    match previous {
                        Some(row)
                            if row.line.wrapped == wrapped
                                && row.line.cells == self.scratch_cells =>
                        {
                            Arc::clone(&row.line)
                        }
                        _ => Arc::new(Line {
                            cells: self.scratch_cells.clone(),
                            wrapped,
                        }),
                    }
                };
                cache.push(CachedRow {
                    id: id.bits,
                    selection,
                    line: Arc::clone(&line),
                });
                lines.push(line);
            }
        }
        if lines.len() < rows {
            let blank = Arc::new(Line {
                cells: vec![Cell::default(); cols],
                wrapped: false,
            });
            lines.resize(rows, blank);
        }
        self.rows = cache;
        lines
    }

    /// Reads the row the iterator is on into `scratch_cells`; whether it
    /// soft-wraps.
    fn read_row(&mut self, view: &View, selection: Option<(u16, u16)>) -> bool {
        let mut raw: sys::GhosttyRow = 0;
        ffi::row_get(self.row_iter, ffi::ROW_RAW, &mut raw);
        let wrapped = ffi::raw_row(raw, ffi::RAW_ROW_WRAP);
        let links = ffi::raw_row(raw, ffi::RAW_ROW_HYPERLINK);
        self.scratch_cells.clear();
        if ffi::row_get(self.row_iter, ffi::ROW_CELLS, &mut self.row_cells) {
            let mut x = 0u16;
            while x < view.key.cols && ffi::cells_next(self.row_cells) {
                let selected = selection.is_some_and(|(start, end)| start <= x && x <= end);
                let cell = read_cell(
                    self.row_cells,
                    &mut self.scratch_codepoints,
                    view,
                    links,
                    selected.then_some(self.selection_colors),
                );
                self.scratch_cells.push(cell);
                x += 1;
            }
        }
        self.scratch_cells
            .resize(usize::from(view.key.cols), Cell::default());
        wrapped
    }

    /// Encodes a key event for the program, honouring its keyboard modes
    /// (DECCKM, keypad, kitty protocol...). Empty when the event produces
    /// nothing (a bare modifier, a release in legacy mode).
    pub fn key(&mut self, input: &KeyInput<'_>) -> Vec<u8> {
        // SAFETY: both handles are live.
        unsafe { sys::ghostty_key_encoder_setopt_from_terminal(self.key_encoder, self.term) };
        let option_as_alt: sys::GhosttyOptionAsAlt = if input.option_as_alt {
            sys::GHOSTTY_OPTION_AS_ALT_TRUE
        } else {
            sys::GHOSTTY_OPTION_AS_ALT_FALSE
        };
        // SAFETY: the encoder is live and the value is the `GhosttyOptionAsAlt`
        // this option takes; the encoder copies it.
        unsafe {
            sys::ghostty_key_encoder_setopt(
                self.key_encoder,
                sys::GHOSTTY_KEY_ENCODER_OPT_MACOS_OPTION_AS_ALT,
                (&raw const option_as_alt).cast(),
            );
        }

        let text = input.text.filter(|text| {
            !text.is_empty()
                && !text
                    .chars()
                    .any(|c| c.is_control() || ('\u{F700}'..='\u{F8FF}').contains(&c))
        });
        let action = match input.action {
            KeyAction::Press => sys::GHOSTTY_KEY_ACTION_PRESS,
            KeyAction::Repeat => sys::GHOSTTY_KEY_ACTION_REPEAT,
            KeyAction::Release => sys::GHOSTTY_KEY_ACTION_RELEASE,
        };
        let consumed = if text.is_some() {
            input.consumed
        } else {
            Mods::NONE
        };
        let event = self.key_event;
        // SAFETY: (the six setters) the event handle is live; values are
        // plain data.
        unsafe { sys::ghostty_key_event_set_action(event, action) };
        // SAFETY: as above.
        unsafe { sys::ghostty_key_event_set_key(event, raw_key(input.key)) };
        // SAFETY: as above.
        unsafe { sys::ghostty_key_event_set_mods(event, raw_mods(input.mods)) };
        // SAFETY: as above.
        unsafe { sys::ghostty_key_event_set_consumed_mods(event, raw_mods(consumed)) };
        // SAFETY: as above.
        unsafe { sys::ghostty_key_event_set_composing(event, false) };
        // SAFETY: as above.
        unsafe {
            sys::ghostty_key_event_set_unshifted_codepoint(
                event,
                input.unshifted.map_or(0, u32::from),
            );
        }
        let (text_ptr, text_len) =
            text.map_or((ptr::null(), 0), |text| (text.as_ptr(), text.len()));
        // SAFETY: the event borrows the text; it lives until the end of this
        // function, and the borrow is dropped again below before returning.
        unsafe { sys::ghostty_key_event_set_utf8(event, text_ptr.cast(), text_len) };

        let encoder = self.key_encoder;
        let out = ffi::encode(64, |buf, cap, len| {
            // SAFETY: both handles are live and `buf` has `cap` writable
            // bytes.
            unsafe { sys::ghostty_key_encoder_encode(encoder, event, buf, cap, len) }
        });
        // SAFETY: the event is live; this ends its borrow of `text`.
        unsafe { sys::ghostty_key_event_set_utf8(event, ptr::null(), 0) };
        out
    }

    /// Encodes a mouse event for the program. Empty when the program did not
    /// ask for mouse reports, or for this kind of event, or when the pointer
    /// moved within the same cell.
    pub fn mouse(&mut self, input: &MouseInput) -> Vec<u8> {
        if let Some(button) = input.button.filter(|button| !button.is_wheel()) {
            let bit = 1u8 << (button as u8);
            match input.action {
                MouseAction::Press => self.buttons_down |= bit,
                MouseAction::Release => self.buttons_down &= !bit,
                MouseAction::Motion => {}
            }
        }
        if !self.mouse_tracking() {
            self.last_mouse_cell = None;
            return Vec::new();
        }

        // The encoder takes whole-pixel cell sizes, and a fractional cell
        // width drifts by a column every few dozen cells. Describe an
        // equivalent grid of whole-pixel cells and no padding instead, and
        // map the pointer into it.
        let (cols, rows) = self.size();
        let geometry = input.geometry;
        let cell_w = (geometry.cell_w.round() as u32).max(1);
        let cell_h = (geometry.cell_h.round() as u32).max(1);
        let width = u32::from(cols) * cell_w;
        let height = u32::from(rows) * cell_h;
        let scale = |pos: f32, pad: f32, cell: f32, whole: u32, extent: u32| -> f32 {
            let cells = if cell > 0.0 { (pos - pad) / cell } else { 0.0 };
            (cells * whole as f32).clamp(0.0, (extent - 1) as f32)
        };
        let x = scale(input.x, geometry.pad_x, geometry.cell_w, cell_w, width);
        let y = scale(input.y, geometry.pad_y, geometry.cell_h, cell_h, height);
        let cell = (
            ((x as u32) / cell_w).min(u32::from(cols) - 1) as u16,
            ((y as u32) / cell_h).min(u32::from(rows) - 1) as u16,
        );
        let same_cell = self.last_mouse_cell.replace(cell) == Some(cell);
        if input.action == MouseAction::Motion && same_cell && !self.mode(MODE_MOUSE_SGR_PIXELS) {
            return Vec::new();
        }

        // SAFETY: both handles are live.
        unsafe { sys::ghostty_mouse_encoder_setopt_from_terminal(self.mouse_encoder, self.term) };
        let size = sys::GhosttyMouseEncoderSize {
            size: size_of::<sys::GhosttyMouseEncoderSize>(),
            screen_width: width,
            screen_height: height,
            cell_width: cell_w,
            cell_height: cell_h,
            ..Default::default()
        };
        // SAFETY: the encoder is live and the value is the sized struct this
        // option takes; the encoder copies it.
        unsafe {
            sys::ghostty_mouse_encoder_setopt(
                self.mouse_encoder,
                sys::GHOSTTY_MOUSE_ENCODER_OPT_SIZE,
                (&raw const size).cast(),
            );
        }
        let any_pressed = self.buttons_down != 0;
        // SAFETY: the encoder is live and the value is the `bool` this option
        // takes.
        unsafe {
            sys::ghostty_mouse_encoder_setopt(
                self.mouse_encoder,
                sys::GHOSTTY_MOUSE_ENCODER_OPT_ANY_BUTTON_PRESSED,
                (&raw const any_pressed).cast(),
            );
        }

        let event = self.mouse_event;
        let action = match input.action {
            MouseAction::Press => sys::GHOSTTY_MOUSE_ACTION_PRESS,
            MouseAction::Release => sys::GHOSTTY_MOUSE_ACTION_RELEASE,
            MouseAction::Motion => sys::GHOSTTY_MOUSE_ACTION_MOTION,
        };
        // SAFETY: (the setters below) the event handle is live; values are
        // plain data.
        unsafe { sys::ghostty_mouse_event_set_action(event, action) };
        match input.button {
            Some(button) => {
                let button = match button {
                    MouseButton::Left => sys::GHOSTTY_MOUSE_BUTTON_LEFT,
                    MouseButton::Right => sys::GHOSTTY_MOUSE_BUTTON_RIGHT,
                    MouseButton::Middle => sys::GHOSTTY_MOUSE_BUTTON_MIDDLE,
                    MouseButton::WheelUp => sys::GHOSTTY_MOUSE_BUTTON_FOUR,
                    MouseButton::WheelDown => sys::GHOSTTY_MOUSE_BUTTON_FIVE,
                    MouseButton::WheelLeft => sys::GHOSTTY_MOUSE_BUTTON_SIX,
                    MouseButton::WheelRight => sys::GHOSTTY_MOUSE_BUTTON_SEVEN,
                };
                // SAFETY: as above.
                unsafe { sys::ghostty_mouse_event_set_button(event, button) };
            }
            None => {
                // SAFETY: as above.
                unsafe { sys::ghostty_mouse_event_clear_button(event) };
            }
        }
        // SAFETY: as above.
        unsafe { sys::ghostty_mouse_event_set_mods(event, raw_mods(input.mods)) };
        // SAFETY: as above.
        unsafe { sys::ghostty_mouse_event_set_position(event, sys::GhosttyMousePosition { x, y }) };

        let encoder = self.mouse_encoder;
        ffi::encode(64, |buf, cap, len| {
            // SAFETY: both handles are live and `buf` has `cap` writable
            // bytes.
            unsafe { sys::ghostty_mouse_encoder_encode(encoder, event, buf, cap, len) }
        })
    }

    /// Encodes pasted text: wrapped in bracketed-paste markers when the
    /// program asked for them (mode 2004), with bytes that could break out of
    /// the paste removed; otherwise with newlines turned into carriage
    /// returns.
    #[must_use]
    pub fn paste(&self, text: &str) -> Vec<u8> {
        if text.is_empty() {
            return Vec::new();
        }
        let bracketed = self.bracketed_paste();
        // The encoder sanitizes its input in place.
        let mut data = text.as_bytes().to_vec();
        ffi::encode(data.len() + 16, |buf, cap, len| {
            // SAFETY: `data` is a writable buffer of the given length and
            // `buf` has `cap` writable bytes.
            unsafe {
                sys::ghostty_paste_encode(
                    data.as_mut_ptr().cast(),
                    data.len(),
                    bracketed,
                    buf,
                    cap,
                    len,
                )
            }
        })
    }

    /// Whether pasting `text` is harmless: no newline that would run a
    /// command and no sequence that ends bracketed paste early. Ask before
    /// pasting when this is `false`.
    #[must_use]
    pub fn paste_is_safe(text: &str) -> bool {
        // SAFETY: the pointer/length describe `text`.
        unsafe { sys::ghostty_paste_is_safe(text.as_ptr().cast(), text.len()) }
    }

    /// The focus report to send when the widget gains or loses focus; empty
    /// unless the program asked for them (mode 1004).
    #[must_use]
    pub fn focus(&self, focused: bool) -> Vec<u8> {
        if !self.mode(MODE_FOCUS_EVENTS) {
            return Vec::new();
        }
        let event = if focused {
            sys::GHOSTTY_FOCUS_GAINED
        } else {
            sys::GHOSTTY_FOCUS_LOST
        };
        ffi::encode(8, |buf, cap, len| {
            // SAFETY: `buf` has `cap` writable bytes.
            unsafe { sys::ghostty_focus_encode(event, buf, cap, len) }
        })
    }

    /// Scrolls the viewport through the scrollback.
    pub fn scroll(&mut self, by: Scroll) {
        let (_, rows) = self.size();
        let rows = isize::try_from(rows).unwrap_or(1);
        let (tag, value) = match by {
            Scroll::Lines(delta) => (
                sys::GHOSTTY_SCROLL_VIEWPORT_DELTA,
                sys::GhosttyTerminalScrollViewportValue { delta },
            ),
            Scroll::Pages(pages) => (
                sys::GHOSTTY_SCROLL_VIEWPORT_DELTA,
                sys::GhosttyTerminalScrollViewportValue {
                    delta: pages.saturating_mul(rows),
                },
            ),
            Scroll::Top => (
                sys::GHOSTTY_SCROLL_VIEWPORT_TOP,
                sys::GhosttyTerminalScrollViewportValue { delta: 0 },
            ),
            Scroll::Bottom => (
                sys::GHOSTTY_SCROLL_VIEWPORT_BOTTOM,
                sys::GhosttyTerminalScrollViewportValue { delta: 0 },
            ),
            Scroll::ToRow(row) => (
                sys::GHOSTTY_SCROLL_VIEWPORT_ROW,
                sys::GhosttyTerminalScrollViewportValue { row },
            ),
        };
        self.term().scroll_viewport(tag, value);
        self.stale = true;
        self.live = true;
        self.search_fresh = false;
    }

    /// Feeds one step of a mouse selection: press (a second and third press
    /// in place select the word, then the line), drag, release.
    pub fn select(&mut self, step: SelectStep, at: SelectPoint) {
        let term = self.term();
        let (cols, rows) = term.size();
        let event = match step {
            SelectStep::Press => self.press_event,
            SelectStep::Drag => self.drag_event,
            SelectStep::Release => self.release_event,
        };
        let cell = term.grid_ref(
            sys::GHOSTTY_POINT_TAG_VIEWPORT,
            at.col.min(cols.saturating_sub(1)),
            u32::from(at.row.min(rows.saturating_sub(1))),
        );
        match cell {
            Some(cell) => ffi::gesture_set(event, ffi::G_REF, &cell),
            // Never leave an older ref behind: it may point at freed pages.
            None => ffi::gesture_clear(event, ffi::G_REF),
        }
        if cell.is_none() && step != SelectStep::Release {
            return;
        }

        let subpixel = |pixels: f32| f64::from(pixels) * SUBPIXEL;
        let position = sys::GhosttySurfacePosition {
            x: subpixel(at.x),
            y: subpixel(at.y),
        };
        match step {
            SelectStep::Press => {
                let now = u64::try_from(self.epoch.elapsed().as_nanos()).unwrap_or(u64::MAX);
                ffi::gesture_set(event, ffi::G_POSITION, &position);
                ffi::gesture_set(event, ffi::G_TIME_NS, &now);
                ffi::gesture_set(event, ffi::G_REPEAT_INTERVAL_NS, &MULTI_CLICK_NS);
                ffi::gesture_set(
                    event,
                    ffi::G_REPEAT_DISTANCE,
                    &subpixel(at.geometry.cell_w.max(4.0)),
                );
            }
            SelectStep::Drag => {
                let whole = |value: f64| value.round().clamp(1.0, f64::from(u32::MAX)) as u32;
                let geometry = sys::GhosttySelectionGestureGeometry {
                    columns: u32::from(cols),
                    cell_width: whole(subpixel(at.geometry.cell_w)),
                    padding_left: subpixel(at.geometry.pad_x).round().max(0.0) as u32,
                    screen_height: whole(subpixel(
                        at.geometry.pad_y + f32::from(rows) * at.geometry.cell_h,
                    )),
                };
                ffi::gesture_set(event, ffi::G_POSITION, &position);
                ffi::gesture_set(event, ffi::G_GEOMETRY, &geometry);
                ffi::gesture_set(event, ffi::G_RECTANGLE, &at.rectangle);
            }
            SelectStep::Release => {}
        }

        let selection = ffi::gesture_apply(self.gesture, term, event);
        // The event's ref is only good until the terminal changes.
        ffi::gesture_clear(event, ffi::G_REF);
        match (step, selection) {
            (SelectStep::Press | SelectStep::Drag, Some(selection)) => {
                term.set(ffi::O_SELECTION, Some(&selection));
            }
            // A plain click selects nothing and drops what was selected.
            (SelectStep::Press, None) => {
                term.set(ffi::O_SELECTION, None);
            }
            (SelectStep::Drag | SelectStep::Release, _) => {}
        }
        self.stale = true;
        self.live = true;
    }

    /// Selects the whole screen and scrollback.
    pub fn select_all(&mut self) {
        let term = self.term();
        if let Some(selection) = term.all() {
            term.set(ffi::O_SELECTION, Some(&selection));
        }
        self.stale = true;
        self.live = true;
    }

    /// Drops the selection.
    pub fn clear_selection(&mut self) {
        self.term().set(ffi::O_SELECTION, None);
        self.stale = true;
        self.live = true;
    }

    /// Whether anything is selected.
    #[must_use]
    pub fn has_selection(&self) -> bool {
        self.term().selection().is_some()
    }

    /// The selected text, soft-wrapped lines joined and trailing blanks
    /// trimmed.
    #[must_use]
    pub fn selection_text(&self) -> Option<String> {
        self.has_selection()
            .then(|| self.term().selection_text(None, true))
            .flatten()
    }

    /// The OSC 8 hyperlink target under a viewport cell.
    #[must_use]
    pub fn link_at(&self, col: u16, row: u16) -> Option<String> {
        let cell = self
            .term()
            .grid_ref(sys::GHOSTTY_POINT_TAG_VIEWPORT, col, u32::from(row))?;
        ffi::grid_ref_link(&cell)
    }

    /// The text of one viewport row, trailing blanks trimmed.
    #[must_use]
    pub fn row_text(&self, row: u16) -> String {
        let term = self.term();
        let (cols, _) = term.size();
        let row = u32::from(row);
        let tag = sys::GHOSTTY_POINT_TAG_VIEWPORT;
        let mut text = term
            .span((tag, 0, row), (tag, cols.saturating_sub(1), row))
            .and_then(|span| term.selection_text(Some(&span), false))
            .unwrap_or_default();
        text.truncate(text.trim_end_matches(['\n', '\r']).len());
        text
    }

    /// The screen as plain text, preceded by up to `history_lines` rows of
    /// scrollback. One line per row (soft wraps are not joined), trailing
    /// blanks trimmed from each, trailing blank lines dropped. Ignores the
    /// viewport's scroll position.
    #[must_use]
    pub fn text(&self, history_lines: usize) -> String {
        plain_text(self.term(), history_lines)
    }

    /// VT bytes that reproduce this terminal in a fresh one of the same
    /// size: up to `history_lines` rows of scrollback, the screen (both of
    /// them when the alternate screen is showing), cursor, pen, modes, tab
    /// stops and title. For attaching a new viewer to a running session.
    ///
    /// The palette is left out on purpose: the viewer keeps its own theme.
    #[must_use]
    pub fn repaint(&self, history_lines: usize) -> Vec<u8> {
        let term = self.term();
        let mut out = REPAINT_RESET.to_vec();
        if term.alt_screen() {
            // Rebuild the primary screen underneath first, from a copy
            // switched back to it (CAN ends any sequence the program left
            // unfinished).
            if let Some(copy) = term.snapshot_copy() {
                copy.0.write(b"\x18\x1b[?1049l");
                paint_screen(copy.0, history_lines, &mut out);
            }
            out.extend_from_slice(b"\x1b[?1049h\x1b[!p");
            paint_screen(term, 0, &mut out);
        } else {
            paint_screen(term, history_lines, &mut out);
        }
        if let Some(title) = term.string(ffi::T_TITLE) {
            out.extend_from_slice(b"\x1b]2;");
            out.extend(title.bytes().filter(|byte| !byte.is_ascii_control()));
            out.extend_from_slice(b"\x1b\\");
        }
        out
    }

    /// Starts, changes or (with `None` or an empty string) ends a search of
    /// the screen and scrollback. ASCII letters match case-insensitively.
    /// Nothing is searched yet: call [`Vt::search_tick`].
    pub fn set_needle(&mut self, needle: Option<&str>) {
        match needle.filter(|needle| !needle.is_empty()) {
            Some(needle) => {
                let value = sys::GhosttyString {
                    ptr: needle.as_ptr(),
                    len: needle.len(),
                };
                self.search_active =
                    ffi::search_set(self.search, sys::GHOSTTY_SEARCH_OPT_NEEDLE, Some(&value));
            }
            None => {
                ffi::search_set::<sys::GhosttyString>(
                    self.search,
                    sys::GHOSTTY_SEARCH_OPT_NEEDLE,
                    None,
                );
                self.search_active = false;
            }
        }
        self.search_fresh = false;
        self.stale = true;
    }

    /// Does a bounded slice of search work and reports progress. Never
    /// blocks: call it once per tick while a search is open, also after it
    /// completes, since that is how the search sees new output.
    pub fn search_tick(&mut self) -> SearchStatus {
        if !self.search_active {
            return SearchStatus {
                total: 0,
                selected: None,
                complete: true,
            };
        }
        let before = self.search_counts();
        if !self.search_fresh {
            self.feed_search();
        }
        let mut status = ffi::search_tick(self.search);
        if status == sys::GHOSTTY_SEARCH_STATUS_FEED_REQUIRED {
            self.feed_search();
            status = ffi::search_tick(self.search);
        }
        let (total, selected) = self.search_counts();
        if (total, selected) != before {
            self.stale = true;
        }
        SearchStatus {
            total,
            selected,
            complete: status == sys::GHOSTTY_SEARCH_STATUS_COMPLETE,
        }
    }

    /// Selects the next match toward older output (wrapping) and scrolls it
    /// into view.
    pub fn search_next(&mut self) -> SearchStatus {
        self.search_step(sys::GHOSTTY_SEARCH_OPT_SELECT_NEXT)
    }

    /// Selects the next match toward newer output (wrapping) and scrolls it
    /// into view.
    pub fn search_prev(&mut self) -> SearchStatus {
        self.search_step(sys::GHOSTTY_SEARCH_OPT_SELECT_PREV)
    }

    fn search_step(&mut self, option: sys::GhosttySearchOption) -> SearchStatus {
        if self.search_active {
            ffi::search_set::<sys::GhosttyString>(self.search, option, None);
            // Selecting may have scrolled the viewport.
            self.search_fresh = false;
            self.stale = true;
            self.live = true;
        }
        self.search_tick()
    }

    /// The parts of the viewport covered by matches, for highlighting.
    pub fn search_matches(&mut self) -> Vec<SearchMatch> {
        if !self.search_active {
            return Vec::new();
        }
        // Match refs point into the terminal's pages and die with its next
        // mutation; a feed re-derives them.
        if !self.search_fresh {
            self.feed_search();
        }
        let term = self.term();
        let (cols, rows) = term.size();

        let mut matches: Vec<sys::GhosttySelection> = Vec::new();
        let mut buffer = sys::GhosttySelectionBuffer {
            ptr: ptr::null_mut(),
            cap: 0,
            len: 0,
        };
        let result = ffi::search_get(self.search, ffi::S_VIEWPORT_MATCHES, &mut buffer);
        if result == sys::GHOSTTY_OUT_OF_SPACE && buffer.len > 0 {
            matches.resize(buffer.len, ffi::sized_selection());
            buffer.ptr = matches.as_mut_ptr();
            buffer.cap = matches.len();
            buffer.len = 0;
            let result = ffi::search_get(self.search, ffi::S_VIEWPORT_MATCHES, &mut buffer);
            if !ffi::ok(result) {
                return Vec::new();
            }
            matches.truncate(buffer.len);
        }

        let screen = sys::GHOSTTY_POINT_TAG_SCREEN;
        let viewport = sys::GHOSTTY_POINT_TAG_VIEWPORT;
        let mut selected = ffi::sized_selection();
        let selected = ffi::ok(ffi::search_get(
            self.search,
            ffi::S_SELECTED_MATCH,
            &mut selected,
        ))
        .then(|| term.point(&selected.start, screen))
        .flatten();

        let last_col = cols.saturating_sub(1);
        let last_row = u32::from(rows.saturating_sub(1));
        let mut out = Vec::new();
        for found in &matches {
            let start = term.point(&found.start, viewport);
            let end = term.point(&found.end, viewport);
            if start.is_none() && end.is_none() {
                continue;
            }
            let (start_x, start_y) = start.unwrap_or((0, 0));
            let (mut end_x, end_y) = end.unwrap_or((last_col, last_row));
            if start_y > last_row || end_y < start_y {
                continue;
            }
            let end_y = end_y.min(last_row);
            let ends_wide = end.is_some()
                && ffi::grid_ref_cell(&found.end).is_some_and(|cell| {
                    ffi::raw_cell(cell, ffi::CELL_WIDE) == sys::GHOSTTY_CELL_WIDE_WIDE
                });
            if ends_wide {
                end_x = end_x.saturating_add(1).min(last_col);
            }
            let is_selected = selected.is_some() && term.point(&found.start, screen) == selected;
            for y in start_y..=end_y {
                let col_start = if y == start_y { start_x } else { 0 };
                let col_end = if y == end_y {
                    end_x.saturating_add(1)
                } else {
                    cols
                };
                if col_start < col_end {
                    out.push(SearchMatch {
                        row: u16::try_from(y).unwrap_or(u16::MAX),
                        col_start,
                        col_end,
                        selected: is_selected,
                    });
                }
            }
        }
        out
    }

    fn feed_search(&mut self) {
        ffi::search_feed(self.search);
        self.search_fresh = true;
    }

    fn search_counts(&self) -> (usize, Option<usize>) {
        let mut total = 0usize;
        ffi::search_get(self.search, ffi::S_TOTAL, &mut total);
        let mut index = 0usize;
        let selected = ffi::ok(ffi::search_get(
            self.search,
            ffi::S_SELECTED_INDEX,
            &mut index,
        ))
        .then_some(index);
        (total, selected)
    }
}

impl Drop for Vt {
    fn drop(&mut self) {
        // Dependents before what they point into: the search and the gesture
        // hold references into the terminal, the row cursors into the render
        // state. Handles are NULL when `new` failed before creating them.
        if !self.search.is_null() {
            // SAFETY: the handle is live, its terminal still is, and neither
            // is used after this.
            unsafe { sys::ghostty_search_free(self.search) };
        }
        for event in [self.press_event, self.drag_event, self.release_event] {
            if !event.is_null() {
                // SAFETY: the handle is live and not used after this.
                unsafe { sys::ghostty_selection_gesture_event_free(event) };
            }
        }
        if !self.gesture.is_null() && !self.term.is_null() {
            // SAFETY: both handles are live; the gesture releases its
            // tracked references in the terminal and is not used after this.
            unsafe { sys::ghostty_selection_gesture_free(self.gesture, self.term) };
        }
        if !self.mouse_event.is_null() {
            // SAFETY: the handle is live and not used after this.
            unsafe { sys::ghostty_mouse_event_free(self.mouse_event) };
        }
        if !self.mouse_encoder.is_null() {
            // SAFETY: the handle is live and not used after this.
            unsafe { sys::ghostty_mouse_encoder_free(self.mouse_encoder) };
        }
        if !self.key_event.is_null() {
            // SAFETY: the handle is live and not used after this.
            unsafe { sys::ghostty_key_event_free(self.key_event) };
        }
        if !self.key_encoder.is_null() {
            // SAFETY: the handle is live and not used after this.
            unsafe { sys::ghostty_key_encoder_free(self.key_encoder) };
        }
        if !self.row_cells.is_null() {
            // SAFETY: the handle is live and not used after this.
            unsafe { sys::ghostty_render_state_row_cells_free(self.row_cells) };
        }
        if !self.row_iter.is_null() {
            // SAFETY: the handle is live and not used after this.
            unsafe { sys::ghostty_render_state_row_iterator_free(self.row_iter) };
        }
        if !self.term.is_null() {
            // SAFETY: the handle is live and not used after this. Freeing
            // runs no callback that outlives the call.
            unsafe { sys::ghostty_terminal_free(self.term) };
        }
        if !self.render.is_null() {
            // SAFETY: the handle is live and not used after this.
            unsafe { sys::ghostty_render_state_free(self.render) };
        }
        // SAFETY: the pointer came from `Box::leak` in `new`, is freed only
        // here, and the terminal that held it as userdata is gone.
        drop(unsafe { Box::from_raw(self.shared.as_ptr()) });
    }
}

fn same_palette(a: &[sys::GhosttyColorRgb; 256], b: &[sys::GhosttyColorRgb; 256]) -> bool {
    a.iter()
        .zip(b)
        .all(|(a, b)| a.r == b.r && a.g == b.g && a.b == b.b)
}

/// Reads the cell the cursor is on. `selected` is `Some` for a cell inside
/// the selection and carries the theme's selection colors, if it has any.
fn read_cell(
    cells: sys::GhosttyRenderStateRowCells,
    codepoints: &mut Vec<u32>,
    view: &View,
    row_has_links: bool,
    selected: Option<Option<(Rgb, Option<Rgb>)>>,
) -> Cell {
    let mut raw: sys::GhosttyCell = 0;
    ffi::cells_get(cells, ffi::C_RAW, &mut raw);
    let mut cell = Cell::default();

    let tag = ffi::raw_cell(raw, ffi::CELL_CONTENT_TAG);
    if tag == sys::GHOSTTY_CELL_CONTENT_CODEPOINT {
        cell.text = char::from_u32(ffi::raw_cell(raw, ffi::CELL_CODEPOINT))
            .filter(|c| *c != '\0' && *c != KITTY_PLACEHOLDER)
            .map_or(Grapheme::EMPTY, Grapheme::from_char);
    } else if tag == sys::GHOSTTY_CELL_CONTENT_CODEPOINT_GRAPHEME {
        ffi::cells_graphemes(cells, codepoints);
        cell.text = Grapheme::from_chars(codepoints.iter().filter_map(|c| char::from_u32(*c)));
    }

    let wide = ffi::raw_cell(raw, ffi::CELL_WIDE);
    if wide == sys::GHOSTTY_CELL_WIDE_WIDE {
        cell.attrs.insert(Attrs::WIDE);
    } else if wide == sys::GHOSTTY_CELL_WIDE_SPACER_TAIL
        || wide == sys::GHOSTTY_CELL_WIDE_SPACER_HEAD
    {
        cell.attrs.insert(Attrs::SPACER);
        cell.text = Grapheme::EMPTY;
    }

    let mut color = sys::GhosttyColorRgb::default();
    if ffi::cells_get(cells, ffi::C_FG, &mut color) {
        cell.fg = Some(ffi::rgb(color));
    }
    if ffi::cells_get(cells, ffi::C_BG, &mut color) {
        cell.bg = Some(ffi::rgb(color));
    }

    let mut style = sys::GhosttyStyle {
        size: size_of::<sys::GhosttyStyle>(),
        ..Default::default()
    };
    if ffi::raw_cell(raw, ffi::CELL_HAS_STYLING) && ffi::cells_get(cells, ffi::C_STYLE, &mut style)
    {
        for (set, flag) in [
            (style.bold, Attrs::BOLD),
            (style.italic, Attrs::ITALIC),
            (style.faint, Attrs::FAINT),
            (style.strikethrough, Attrs::STRIKE),
            (style.invisible, Attrs::INVISIBLE),
            (style.blink, Attrs::BLINK),
            (style.overline, Attrs::OVERLINE),
        ] {
            if set {
                cell.attrs.insert(flag);
            }
        }
        cell.underline = match style.underline {
            sys::GHOSTTY_SGR_UNDERLINE_SINGLE => Underline::Single,
            sys::GHOSTTY_SGR_UNDERLINE_DOUBLE => Underline::Double,
            sys::GHOSTTY_SGR_UNDERLINE_CURLY => Underline::Curly,
            sys::GHOSTTY_SGR_UNDERLINE_DOTTED => Underline::Dotted,
            sys::GHOSTTY_SGR_UNDERLINE_DASHED => Underline::Dashed,
            _ => Underline::None,
        };
        cell.underline_color = ffi::style_color(&style.underline_color, &view.palette);
        if style.inverse {
            let fg = cell.fg.unwrap_or(view.key.foreground);
            let bg = cell.bg.unwrap_or(view.key.background);
            cell.fg = Some(bg);
            cell.bg = Some(fg);
        }
    }

    if row_has_links && ffi::raw_cell(raw, ffi::CELL_HAS_HYPERLINK) {
        cell.attrs.insert(Attrs::LINK);
    }
    if let Some(colors) = selected {
        cell.attrs.insert(Attrs::SELECTED);
        if let Some((bg, fg)) = colors {
            cell.bg = Some(bg);
            cell.fg = fg.or(cell.fg);
        }
    }
    cell
}

/// The screen of `term`, preceded by up to `history_lines` scrollback rows,
/// as plain text.
fn plain_text(term: Term, history_lines: usize) -> String {
    let (cols, rows) = term.size();
    let scrollback = term.get(ffi::T_SCROLLBACK_ROWS).unwrap_or(0);
    let history = history_lines.min(scrollback);
    let active = sys::GHOSTTY_POINT_TAG_ACTIVE;
    let start = if history > 0 {
        (
            sys::GHOSTTY_POINT_TAG_HISTORY,
            0,
            u32::try_from(scrollback - history).unwrap_or(u32::MAX),
        )
    } else {
        (active, 0, 0)
    };
    let end = (
        active,
        cols.saturating_sub(1),
        u32::from(rows.saturating_sub(1)),
    );
    let mut text = term
        .span(start, end)
        .and_then(|span| term.selection_text(Some(&span), false))
        .unwrap_or_default();
    text.truncate(text.trim_end_matches(['\n', '\r', ' ']).len());
    text
}

/// Appends VT bytes that rebuild the active screen of `term` (and
/// `history_lines` rows of its scrollback) in a cleared terminal of the same
/// size.
fn paint_screen(term: Term, history_lines: usize, out: &mut Vec<u8>) {
    let (cols, rows) = term.size();
    let last_col = cols.saturating_sub(1);
    let scrollback = term.get(ffi::T_SCROLLBACK_ROWS).unwrap_or(0);
    let history = history_lines.min(scrollback);
    if history > 0 {
        let tag = sys::GHOSTTY_POINT_TAG_HISTORY;
        let first = u32::try_from(scrollback - history).unwrap_or(u32::MAX);
        let last = u32::try_from(scrollback - 1).unwrap_or(u32::MAX);
        if let Some(span) = term.span((tag, 0, first), (tag, last_col, last)) {
            out.extend_from_slice(&term.format_vt(&span, false));
            // Push what was just written off the top into the viewer's
            // scrollback, then start the screen from a clean slate.
            out.extend_from_slice(b"\x1b[0m\r\n");
            out.extend(std::iter::repeat_n(
                b'\n',
                usize::from(rows).saturating_sub(1),
            ));
            out.extend_from_slice(b"\x1b[H\x1b[2J");
        }
    }
    let tag = sys::GHOSTTY_POINT_TAG_ACTIVE;
    let last_row = u32::from(rows.saturating_sub(1));
    if let Some(span) = term.span((tag, 0, 0), (tag, last_col, last_row)) {
        let mut screen = term.format_vt(&span, true);
        for sequence in REPAINT_STRIP {
            remove_all(&mut screen, sequence);
        }
        out.extend_from_slice(&screen);
    }
    // The formatter leaves the cursor shape out (DECSCUSR).
    if let Some(shape) = cursor_shape(term) {
        out.extend_from_slice(format!("\x1b[{shape} q").as_bytes());
    }
}

/// The DECSCUSR parameter for the cursor of `term`'s active screen.
///
/// Only a render state knows the shape, and updating one consumes the
/// terminal's dirty marks, which belong to the `Vt`'s own render state. So
/// this looks at a throwaway copy of the terminal instead.
fn cursor_shape(term: Term) -> Option<u8> {
    let copy = term.snapshot_copy()?;
    let mut state: sys::GhosttyRenderState = ptr::null_mut();
    // SAFETY: the out-pointer is a writable handle slot, and NULL selects the
    // default allocator.
    let created = unsafe { sys::ghostty_render_state_new(ptr::null(), &raw mut state) };
    if !ffi::ok(created) || state.is_null() {
        return None;
    }
    ffi::render_update(state, copy.0);
    let mut cursor = sys::GhosttyRenderStateCursor {
        size: size_of::<sys::GhosttyRenderStateCursor>(),
        ..Default::default()
    };
    let read = ffi::render_get(state, ffi::R_CURSOR, &mut cursor);
    // SAFETY: the handle was created above and is not used after this.
    unsafe { sys::ghostty_render_state_free(state) };
    let shape = match cursor.visual_style {
        sys::GHOSTTY_RENDER_STATE_CURSOR_VISUAL_STYLE_UNDERLINE => 3,
        sys::GHOSTTY_RENDER_STATE_CURSOR_VISUAL_STYLE_BAR => 5,
        _ => 1,
    };
    read.then_some(shape + u8::from(!cursor.blinking))
}

/// Removes every occurrence of `needle` from `bytes`.
fn remove_all(bytes: &mut Vec<u8>, needle: &[u8]) {
    if needle.is_empty() || bytes.len() < needle.len() {
        return;
    }
    let mut kept = Vec::with_capacity(bytes.len());
    let mut rest = bytes.as_slice();
    while let Some(at) = rest
        .windows(needle.len())
        .position(|window| window == needle)
    {
        kept.extend_from_slice(&rest[..at]);
        rest = &rest[at + needle.len()..];
    }
    kept.extend_from_slice(rest);
    *bytes = kept;
}

fn raw_mods(mods: Mods) -> sys::GhosttyMods {
    let mut out = 0;
    for (flag, raw) in [
        (Mods::SHIFT, sys::GHOSTTY_MODS_SHIFT),
        (Mods::CTRL, sys::GHOSTTY_MODS_CTRL),
        (Mods::ALT, sys::GHOSTTY_MODS_ALT),
        (Mods::SUPER, sys::GHOSTTY_MODS_SUPER),
    ] {
        if mods.contains(flag) {
            out |= raw as sys::GhosttyMods;
        }
    }
    out
}

fn raw_key(key: Key) -> sys::GhosttyKey {
    match key {
        Key::A => sys::GHOSTTY_KEY_A,
        Key::B => sys::GHOSTTY_KEY_B,
        Key::C => sys::GHOSTTY_KEY_C,
        Key::D => sys::GHOSTTY_KEY_D,
        Key::E => sys::GHOSTTY_KEY_E,
        Key::F => sys::GHOSTTY_KEY_F,
        Key::G => sys::GHOSTTY_KEY_G,
        Key::H => sys::GHOSTTY_KEY_H,
        Key::I => sys::GHOSTTY_KEY_I,
        Key::J => sys::GHOSTTY_KEY_J,
        Key::K => sys::GHOSTTY_KEY_K,
        Key::L => sys::GHOSTTY_KEY_L,
        Key::M => sys::GHOSTTY_KEY_M,
        Key::N => sys::GHOSTTY_KEY_N,
        Key::O => sys::GHOSTTY_KEY_O,
        Key::P => sys::GHOSTTY_KEY_P,
        Key::Q => sys::GHOSTTY_KEY_Q,
        Key::R => sys::GHOSTTY_KEY_R,
        Key::S => sys::GHOSTTY_KEY_S,
        Key::T => sys::GHOSTTY_KEY_T,
        Key::U => sys::GHOSTTY_KEY_U,
        Key::V => sys::GHOSTTY_KEY_V,
        Key::W => sys::GHOSTTY_KEY_W,
        Key::X => sys::GHOSTTY_KEY_X,
        Key::Y => sys::GHOSTTY_KEY_Y,
        Key::Z => sys::GHOSTTY_KEY_Z,
        Key::Digit0 => sys::GHOSTTY_KEY_DIGIT_0,
        Key::Digit1 => sys::GHOSTTY_KEY_DIGIT_1,
        Key::Digit2 => sys::GHOSTTY_KEY_DIGIT_2,
        Key::Digit3 => sys::GHOSTTY_KEY_DIGIT_3,
        Key::Digit4 => sys::GHOSTTY_KEY_DIGIT_4,
        Key::Digit5 => sys::GHOSTTY_KEY_DIGIT_5,
        Key::Digit6 => sys::GHOSTTY_KEY_DIGIT_6,
        Key::Digit7 => sys::GHOSTTY_KEY_DIGIT_7,
        Key::Digit8 => sys::GHOSTTY_KEY_DIGIT_8,
        Key::Digit9 => sys::GHOSTTY_KEY_DIGIT_9,
        Key::Backquote => sys::GHOSTTY_KEY_BACKQUOTE,
        Key::Backslash => sys::GHOSTTY_KEY_BACKSLASH,
        Key::BracketLeft => sys::GHOSTTY_KEY_BRACKET_LEFT,
        Key::BracketRight => sys::GHOSTTY_KEY_BRACKET_RIGHT,
        Key::Comma => sys::GHOSTTY_KEY_COMMA,
        Key::Equal => sys::GHOSTTY_KEY_EQUAL,
        Key::Minus => sys::GHOSTTY_KEY_MINUS,
        Key::Period => sys::GHOSTTY_KEY_PERIOD,
        Key::Quote => sys::GHOSTTY_KEY_QUOTE,
        Key::Semicolon => sys::GHOSTTY_KEY_SEMICOLON,
        Key::Slash => sys::GHOSTTY_KEY_SLASH,
        Key::Enter => sys::GHOSTTY_KEY_ENTER,
        Key::Tab => sys::GHOSTTY_KEY_TAB,
        Key::Escape => sys::GHOSTTY_KEY_ESCAPE,
        Key::Backspace => sys::GHOSTTY_KEY_BACKSPACE,
        Key::Space => sys::GHOSTTY_KEY_SPACE,
        Key::Delete => sys::GHOSTTY_KEY_DELETE,
        Key::Insert => sys::GHOSTTY_KEY_INSERT,
        Key::Home => sys::GHOSTTY_KEY_HOME,
        Key::End => sys::GHOSTTY_KEY_END,
        Key::PageUp => sys::GHOSTTY_KEY_PAGE_UP,
        Key::PageDown => sys::GHOSTTY_KEY_PAGE_DOWN,
        Key::ArrowUp => sys::GHOSTTY_KEY_ARROW_UP,
        Key::ArrowDown => sys::GHOSTTY_KEY_ARROW_DOWN,
        Key::ArrowLeft => sys::GHOSTTY_KEY_ARROW_LEFT,
        Key::ArrowRight => sys::GHOSTTY_KEY_ARROW_RIGHT,
        Key::F1 => sys::GHOSTTY_KEY_F1,
        Key::F2 => sys::GHOSTTY_KEY_F2,
        Key::F3 => sys::GHOSTTY_KEY_F3,
        Key::F4 => sys::GHOSTTY_KEY_F4,
        Key::F5 => sys::GHOSTTY_KEY_F5,
        Key::F6 => sys::GHOSTTY_KEY_F6,
        Key::F7 => sys::GHOSTTY_KEY_F7,
        Key::F8 => sys::GHOSTTY_KEY_F8,
        Key::F9 => sys::GHOSTTY_KEY_F9,
        Key::F10 => sys::GHOSTTY_KEY_F10,
        Key::F11 => sys::GHOSTTY_KEY_F11,
        Key::F12 => sys::GHOSTTY_KEY_F12,
        Key::Numpad0 => sys::GHOSTTY_KEY_NUMPAD_0,
        Key::Numpad1 => sys::GHOSTTY_KEY_NUMPAD_1,
        Key::Numpad2 => sys::GHOSTTY_KEY_NUMPAD_2,
        Key::Numpad3 => sys::GHOSTTY_KEY_NUMPAD_3,
        Key::Numpad4 => sys::GHOSTTY_KEY_NUMPAD_4,
        Key::Numpad5 => sys::GHOSTTY_KEY_NUMPAD_5,
        Key::Numpad6 => sys::GHOSTTY_KEY_NUMPAD_6,
        Key::Numpad7 => sys::GHOSTTY_KEY_NUMPAD_7,
        Key::Numpad8 => sys::GHOSTTY_KEY_NUMPAD_8,
        Key::Numpad9 => sys::GHOSTTY_KEY_NUMPAD_9,
        Key::NumpadAdd => sys::GHOSTTY_KEY_NUMPAD_ADD,
        Key::NumpadSubtract => sys::GHOSTTY_KEY_NUMPAD_SUBTRACT,
        Key::NumpadMultiply => sys::GHOSTTY_KEY_NUMPAD_MULTIPLY,
        Key::NumpadDivide => sys::GHOSTTY_KEY_NUMPAD_DIVIDE,
        Key::NumpadDecimal => sys::GHOSTTY_KEY_NUMPAD_DECIMAL,
        Key::NumpadEnter => sys::GHOSTTY_KEY_NUMPAD_ENTER,
        Key::NumpadEqual => sys::GHOSTTY_KEY_NUMPAD_EQUAL,
        Key::Unidentified => sys::GHOSTTY_KEY_UNIDENTIFIED,
    }
}
