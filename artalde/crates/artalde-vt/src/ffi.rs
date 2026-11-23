//! Thin, typed helpers over the raw libghostty-vt calls.
//!
//! Everything `unsafe` about reading and writing terminal state through
//! `void*` lives here, behind keys that carry the value type, so the rest of
//! the crate cannot pair a key with the wrong type.
//!
//! Invariant every helper relies on: a handle passed in is one libghostty-vt
//! returned and that has not been freed. [`crate::Vt`] owns its handles and
//! frees them only in `Drop`; the callbacks get theirs from the library.

#![allow(
    unsafe_code,
    reason = "this module is the FFI boundary to libghostty-vt"
)]

use std::ffi::{c_char, c_void};
use std::marker::PhantomData;
use std::ptr;

use crate::sys;
use crate::types::Rgb;

/// Whether a call succeeded.
pub(crate) fn ok(result: sys::GhosttyResult) -> bool {
    result == sys::GHOSTTY_SUCCESS
}

macro_rules! key_type {
    ($(#[$doc:meta])* $name:ident, $raw:ty) => {
        $(#[$doc])*
        ///
        /// `T` is the type the header documents for the key.
        pub(crate) struct $name<T>($raw, PhantomData<fn() -> T>);

        impl<T> $name<T> {
            const fn new(raw: $raw) -> Self {
                Self(raw, PhantomData)
            }
        }

        impl<T> Clone for $name<T> {
            fn clone(&self) -> Self {
                *self
            }
        }

        impl<T> Copy for $name<T> {}
    };
}

key_type!(
    /// A `ghostty_terminal_get` key.
    TermData,
    sys::GhosttyTerminalData
);
key_type!(
    /// A `ghostty_terminal_set` option whose value is passed by pointer.
    TermOpt,
    sys::GhosttyTerminalOption
);
key_type!(
    /// A `ghostty_render_state_get` key.
    RenderData,
    sys::GhosttyRenderStateData
);
key_type!(
    /// A `ghostty_render_state_row_get` key.
    RowData,
    sys::GhosttyRenderStateRowData
);
key_type!(
    /// A `ghostty_render_state_row_cells_get` key.
    CellsData,
    sys::GhosttyRenderStateRowCellsData
);
key_type!(
    /// A `ghostty_cell_get` key.
    RawCellData,
    sys::GhosttyCellData
);
key_type!(
    /// A `ghostty_row_get` key.
    RawRowData,
    sys::GhosttyRowData
);
key_type!(
    /// A `ghostty_search_get` key.
    SearchData,
    sys::GhosttySearchData
);
key_type!(
    /// A `ghostty_selection_gesture_event_set` option.
    GestureOpt,
    sys::GhosttySelectionGestureEventOption
);

// The value types below are the ones documented next to each constant in
// `include/ghostty/vt/*.h`.

pub(crate) const T_COLS: TermData<u16> = TermData::new(sys::GHOSTTY_TERMINAL_DATA_COLS);
pub(crate) const T_ROWS: TermData<u16> = TermData::new(sys::GHOSTTY_TERMINAL_DATA_ROWS);
pub(crate) const T_ACTIVE_SCREEN: TermData<sys::GhosttyTerminalScreen> =
    TermData::new(sys::GHOSTTY_TERMINAL_DATA_ACTIVE_SCREEN);
pub(crate) const T_KITTY_FLAGS: TermData<u8> =
    TermData::new(sys::GHOSTTY_TERMINAL_DATA_KITTY_KEYBOARD_FLAGS);
pub(crate) const T_SCROLLBAR: TermData<sys::GhosttyTerminalScrollbar> =
    TermData::new(sys::GHOSTTY_TERMINAL_DATA_SCROLLBAR);
pub(crate) const T_MOUSE_TRACKING: TermData<bool> =
    TermData::new(sys::GHOSTTY_TERMINAL_DATA_MOUSE_TRACKING);
pub(crate) const T_TITLE: TermData<sys::GhosttyString> =
    TermData::new(sys::GHOSTTY_TERMINAL_DATA_TITLE);
pub(crate) const T_PWD: TermData<sys::GhosttyString> =
    TermData::new(sys::GHOSTTY_TERMINAL_DATA_PWD);
pub(crate) const T_SCROLLBACK_ROWS: TermData<usize> =
    TermData::new(sys::GHOSTTY_TERMINAL_DATA_SCROLLBACK_ROWS);
pub(crate) const T_SELECTION: TermData<sys::GhosttySelection> =
    TermData::new(sys::GHOSTTY_TERMINAL_DATA_SELECTION);
pub(crate) const T_VIEWPORT_ACTIVE: TermData<bool> =
    TermData::new(sys::GHOSTTY_TERMINAL_DATA_VIEWPORT_ACTIVE);
pub(crate) const T_MODE: TermData<sys::GhosttyTerminalModeConfig> =
    TermData::new(sys::GHOSTTY_TERMINAL_DATA_MODE);

pub(crate) const O_FOREGROUND: TermOpt<sys::GhosttyColorRgb> =
    TermOpt::new(sys::GHOSTTY_TERMINAL_OPT_COLOR_FOREGROUND);
pub(crate) const O_BACKGROUND: TermOpt<sys::GhosttyColorRgb> =
    TermOpt::new(sys::GHOSTTY_TERMINAL_OPT_COLOR_BACKGROUND);
pub(crate) const O_CURSOR: TermOpt<sys::GhosttyColorRgb> =
    TermOpt::new(sys::GHOSTTY_TERMINAL_OPT_COLOR_CURSOR);
pub(crate) const O_PALETTE: TermOpt<[sys::GhosttyColorRgb; 256]> =
    TermOpt::new(sys::GHOSTTY_TERMINAL_OPT_COLOR_PALETTE);
pub(crate) const O_SELECTION: TermOpt<sys::GhosttySelection> =
    TermOpt::new(sys::GHOSTTY_TERMINAL_OPT_SELECTION);
pub(crate) const O_SCROLLBACK_MAX_BYTES: TermOpt<usize> =
    TermOpt::new(sys::GHOSTTY_TERMINAL_OPT_SCROLLBACK_MAX_BYTES);
pub(crate) const O_MODE_DEFAULT: TermOpt<sys::GhosttyTerminalModeConfig> =
    TermOpt::new(sys::GHOSTTY_TERMINAL_OPT_MODE_DEFAULT);
pub(crate) const O_MODE: TermOpt<sys::GhosttyTerminalModeConfig> =
    TermOpt::new(sys::GHOSTTY_TERMINAL_OPT_MODE);
pub(crate) const O_CLIPBOARD_WRITE_MAX_BYTES: TermOpt<usize> =
    TermOpt::new(sys::GHOSTTY_TERMINAL_OPT_CLIPBOARD_WRITE_MAX_BYTES);
pub(crate) const O_KITTY_IMAGE_STORAGE_LIMIT: TermOpt<u64> =
    TermOpt::new(sys::GHOSTTY_TERMINAL_OPT_KITTY_IMAGE_STORAGE_LIMIT);

pub(crate) const R_DIRTY: RenderData<sys::GhosttyRenderStateDirty> =
    RenderData::new(sys::GHOSTTY_RENDER_STATE_DATA_DIRTY);
pub(crate) const R_ROW_ITERATOR: RenderData<sys::GhosttyRenderStateRowIterator> =
    RenderData::new(sys::GHOSTTY_RENDER_STATE_DATA_ROW_ITERATOR);
pub(crate) const R_CURSOR: RenderData<sys::GhosttyRenderStateCursor> =
    RenderData::new(sys::GHOSTTY_RENDER_STATE_DATA_CURSOR);
pub(crate) const R_COLORS: RenderData<sys::GhosttyRenderStateColors> =
    RenderData::new(sys::GHOSTTY_RENDER_STATE_DATA_COLORS);
pub(crate) const R_COLS: RenderData<u16> = RenderData::new(sys::GHOSTTY_RENDER_STATE_DATA_COLS);
pub(crate) const R_ROWS: RenderData<u16> = RenderData::new(sys::GHOSTTY_RENDER_STATE_DATA_ROWS);

pub(crate) const ROW_DIRTY: RowData<bool> = RowData::new(sys::GHOSTTY_RENDER_STATE_ROW_DATA_DIRTY);
pub(crate) const ROW_RAW: RowData<sys::GhosttyRow> =
    RowData::new(sys::GHOSTTY_RENDER_STATE_ROW_DATA_RAW);
pub(crate) const ROW_CELLS: RowData<sys::GhosttyRenderStateRowCells> =
    RowData::new(sys::GHOSTTY_RENDER_STATE_ROW_DATA_CELLS);
pub(crate) const ROW_SELECTION: RowData<sys::GhosttyRenderStateRowSelection> =
    RowData::new(sys::GHOSTTY_RENDER_STATE_ROW_DATA_SELECTION);
pub(crate) const ROW_ID: RowData<sys::GhosttyRenderStateRowId> =
    RowData::new(sys::GHOSTTY_RENDER_STATE_ROW_DATA_ID);

pub(crate) const C_RAW: CellsData<sys::GhosttyCell> =
    CellsData::new(sys::GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_RAW);
pub(crate) const C_STYLE: CellsData<sys::GhosttyStyle> =
    CellsData::new(sys::GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_STYLE);
pub(crate) const C_GRAPHEMES_LEN: CellsData<u32> =
    CellsData::new(sys::GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_GRAPHEMES_LEN);
pub(crate) const C_BG: CellsData<sys::GhosttyColorRgb> =
    CellsData::new(sys::GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_BG_COLOR);
pub(crate) const C_FG: CellsData<sys::GhosttyColorRgb> =
    CellsData::new(sys::GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_FG_COLOR);

pub(crate) const CELL_CODEPOINT: RawCellData<u32> =
    RawCellData::new(sys::GHOSTTY_CELL_DATA_CODEPOINT);
pub(crate) const CELL_CONTENT_TAG: RawCellData<sys::GhosttyCellContentTag> =
    RawCellData::new(sys::GHOSTTY_CELL_DATA_CONTENT_TAG);
pub(crate) const CELL_WIDE: RawCellData<sys::GhosttyCellWide> =
    RawCellData::new(sys::GHOSTTY_CELL_DATA_WIDE);
pub(crate) const CELL_HAS_STYLING: RawCellData<bool> =
    RawCellData::new(sys::GHOSTTY_CELL_DATA_HAS_STYLING);
pub(crate) const CELL_HAS_HYPERLINK: RawCellData<bool> =
    RawCellData::new(sys::GHOSTTY_CELL_DATA_HAS_HYPERLINK);

pub(crate) const RAW_ROW_WRAP: RawRowData<bool> = RawRowData::new(sys::GHOSTTY_ROW_DATA_WRAP);
pub(crate) const RAW_ROW_HYPERLINK: RawRowData<bool> =
    RawRowData::new(sys::GHOSTTY_ROW_DATA_HYPERLINK);

pub(crate) const S_TOTAL: SearchData<usize> =
    SearchData::new(sys::GHOSTTY_SEARCH_DATA_TOTAL_MATCHES);
pub(crate) const S_SELECTED_INDEX: SearchData<usize> =
    SearchData::new(sys::GHOSTTY_SEARCH_DATA_SELECTED_INDEX);
pub(crate) const S_SELECTED_MATCH: SearchData<sys::GhosttySelection> =
    SearchData::new(sys::GHOSTTY_SEARCH_DATA_SELECTED_MATCH);
pub(crate) const S_VIEWPORT_MATCHES: SearchData<sys::GhosttySelectionBuffer> =
    SearchData::new(sys::GHOSTTY_SEARCH_DATA_VIEWPORT_MATCHES);

pub(crate) const G_REF: GestureOpt<sys::GhosttyGridRef> =
    GestureOpt::new(sys::GHOSTTY_SELECTION_GESTURE_EVENT_OPT_REF);
pub(crate) const G_POSITION: GestureOpt<sys::GhosttySurfacePosition> =
    GestureOpt::new(sys::GHOSTTY_SELECTION_GESTURE_EVENT_OPT_POSITION);
pub(crate) const G_TIME_NS: GestureOpt<u64> =
    GestureOpt::new(sys::GHOSTTY_SELECTION_GESTURE_EVENT_OPT_TIME_NS);
pub(crate) const G_REPEAT_INTERVAL_NS: GestureOpt<u64> =
    GestureOpt::new(sys::GHOSTTY_SELECTION_GESTURE_EVENT_OPT_REPEAT_INTERVAL_NS);
pub(crate) const G_REPEAT_DISTANCE: GestureOpt<f64> =
    GestureOpt::new(sys::GHOSTTY_SELECTION_GESTURE_EVENT_OPT_REPEAT_DISTANCE);
pub(crate) const G_GEOMETRY: GestureOpt<sys::GhosttySelectionGestureGeometry> =
    GestureOpt::new(sys::GHOSTTY_SELECTION_GESTURE_EVENT_OPT_GEOMETRY);
pub(crate) const G_RECTANGLE: GestureOpt<bool> =
    GestureOpt::new(sys::GHOSTTY_SELECTION_GESTURE_EVENT_OPT_RECTANGLE);

/// The packed `GhosttyMode` of a DEC private mode: the `n` of `CSI ? n h`.
/// Bit 15 marks ANSI modes and stays clear. (The header's `GHOSTTY_MODE_*`
/// names are macros over an inline function, which bindgen cannot export.)
pub(crate) const fn dec_mode(number: u16) -> sys::GhosttyMode {
    number & 0x7FFF
}

/// Converts a libghostty color.
pub(crate) const fn rgb(color: sys::GhosttyColorRgb) -> Rgb {
    Rgb::new(color.r, color.g, color.b)
}

/// Converts to a libghostty color.
pub(crate) const fn raw_rgb(color: Rgb) -> sys::GhosttyColorRgb {
    sys::GhosttyColorRgb {
        r: color.r,
        g: color.g,
        b: color.b,
    }
}

/// A borrowed terminal handle.
#[derive(Clone, Copy)]
pub(crate) struct Term(pub(crate) sys::GhosttyTerminal);

impl Term {
    /// Reads `key` into `out`. `out` keeps its value when the call fails.
    pub(crate) fn get_into<T>(self, key: TermData<T>, out: &mut T) -> sys::GhosttyResult {
        // SAFETY: the handle is live (module invariant) and `out` is a valid
        // `T`, the type the header documents for this key.
        unsafe { sys::ghostty_terminal_get(self.0, key.0, ptr::from_mut(out).cast()) }
    }

    /// Reads `key`, or `None` when the terminal has no value for it.
    pub(crate) fn get<T: Default>(self, key: TermData<T>) -> Option<T> {
        let mut out = T::default();
        ok(self.get_into(key, &mut out)).then_some(out)
    }

    /// Sets `opt`; `None` passes NULL, which resets or clears most options.
    pub(crate) fn set<T>(self, opt: TermOpt<T>, value: Option<&T>) -> bool {
        let value = value.map_or(ptr::null(), |value| ptr::from_ref(value).cast::<c_void>());
        // SAFETY: the handle is live and `value` is NULL or points to a `T`,
        // the type the header documents for this option. The terminal copies
        // the value before returning.
        ok(unsafe { sys::ghostty_terminal_set(self.0, opt.0, value) })
    }

    /// Sets an option whose value is itself the pointer (userdata and the
    /// callbacks).
    pub(crate) fn set_pointer(self, opt: sys::GhosttyTerminalOption, value: *const c_void) -> bool {
        // SAFETY: the handle is live. The callers pass, per option, either the
        // userdata pointer or a function pointer of the callback type the
        // header documents (checked by the `const _` assertions in `vt.rs`).
        ok(unsafe { sys::ghostty_terminal_set(self.0, opt, value) })
    }

    /// Feeds program output.
    pub(crate) fn write(self, bytes: &[u8]) {
        // SAFETY: the handle is live and the pointer/length describe `bytes`.
        unsafe { sys::ghostty_terminal_vt_write(self.0, bytes.as_ptr(), bytes.len()) };
    }

    /// The value of a packed mode.
    pub(crate) fn mode(self, mode: sys::GhosttyMode) -> bool {
        let mut config = sys::GhosttyTerminalModeConfig { mode, value: false };
        ok(self.get_into(T_MODE, &mut config)) && config.value
    }

    /// Whether the alternate screen is showing.
    pub(crate) fn alt_screen(self) -> bool {
        self.get(T_ACTIVE_SCREEN) == Some(sys::GHOSTTY_TERMINAL_SCREEN_ALTERNATE)
    }

    /// `(cols, rows)`.
    pub(crate) fn size(self) -> (u16, u16) {
        (self.get(T_COLS).unwrap_or(0), self.get(T_ROWS).unwrap_or(0))
    }

    /// A string the terminal owns (title, pwd), copied out.
    pub(crate) fn string(self, key: TermData<sys::GhosttyString>) -> Option<String> {
        let value = self.get(key)?;
        if value.ptr.is_null() || value.len == 0 {
            return None;
        }
        // SAFETY: on success the terminal returns a pointer to `len` bytes it
        // owns and keeps until its next mutation; nothing mutates it between
        // the call above and this copy.
        let bytes = unsafe { std::slice::from_raw_parts(value.ptr, value.len) };
        Some(String::from_utf8_lossy(bytes).into_owned())
    }

    /// A reference to the cell at `(x, y)` in the coordinate space `tag`.
    /// Valid until the terminal is next mutated.
    pub(crate) fn grid_ref(
        self,
        tag: sys::GhosttyPointTag,
        x: u16,
        y: u32,
    ) -> Option<sys::GhosttyGridRef> {
        let point = sys::GhosttyPoint {
            tag,
            value: sys::GhosttyPointValue {
                coordinate: sys::GhosttyPointCoordinate { x, y },
            },
        };
        let mut out = sys::GhosttyGridRef {
            size: size_of::<sys::GhosttyGridRef>(),
            ..Default::default()
        };
        // SAFETY: the handle is live and `out` is a sized, writable grid ref.
        let result = unsafe { sys::ghostty_terminal_grid_ref(self.0, point, &raw mut out) };
        ok(result).then_some(out)
    }

    /// Where a grid ref sits in the coordinate space `tag`, if it is there.
    pub(crate) fn point(
        self,
        grid_ref: &sys::GhosttyGridRef,
        tag: sys::GhosttyPointTag,
    ) -> Option<(u16, u32)> {
        let mut out = sys::GhosttyPointCoordinate::default();
        // SAFETY: the handle is live; `grid_ref` came from this terminal and
        // the terminal has not been mutated since (the callers convert refs
        // right after obtaining them).
        let result = unsafe {
            sys::ghostty_terminal_point_from_grid_ref(self.0, grid_ref, tag, &raw mut out)
        };
        ok(result).then_some((out.x, out.y))
    }

    /// The selection spanning two points, both inclusive.
    pub(crate) fn span(
        self,
        start: (sys::GhosttyPointTag, u16, u32),
        end: (sys::GhosttyPointTag, u16, u32),
    ) -> Option<sys::GhosttySelection> {
        Some(sys::GhosttySelection {
            size: size_of::<sys::GhosttySelection>(),
            start: self.grid_ref(start.0, start.1, start.2)?,
            end: self.grid_ref(end.0, end.1, end.2)?,
            rectangle: false,
        })
    }

    /// Formats a selection (`None`: the terminal's current one) as plain text.
    pub(crate) fn selection_text(
        self,
        selection: Option<&sys::GhosttySelection>,
        unwrap: bool,
    ) -> Option<String> {
        let options = sys::GhosttyTerminalSelectionFormatOptions {
            size: size_of::<sys::GhosttyTerminalSelectionFormatOptions>(),
            emit: sys::GHOSTTY_FORMATTER_FORMAT_PLAIN,
            unwrap,
            trim: true,
            selection: selection.map_or(ptr::null(), ptr::from_ref),
        };
        let mut out = ptr::null_mut();
        let mut len = 0usize;
        // SAFETY: the handle is live, `options.selection` is NULL or a
        // selection whose refs were taken from this terminal since its last
        // mutation, and both out-pointers are writable.
        let result = unsafe {
            sys::ghostty_terminal_selection_format_alloc(
                self.0,
                ptr::null(),
                options,
                &raw mut out,
                &raw mut len,
            )
        };
        if !ok(result) {
            return None;
        }
        Some(String::from_utf8_lossy(&take_alloc(out, len)).into_owned())
    }

    /// Formats a selection as VT sequences. `extras` adds what a viewer needs
    /// to continue the session (modes, cursor, pen, tab stops...), never the
    /// palette, which would paint over the viewer's own theme.
    pub(crate) fn format_vt(self, selection: &sys::GhosttySelection, extras: bool) -> Vec<u8> {
        let options = sys::GhosttyFormatterTerminalOptions {
            size: size_of::<sys::GhosttyFormatterTerminalOptions>(),
            emit: sys::GHOSTTY_FORMATTER_FORMAT_VT,
            // Soft-wrapped rows come out as one run, so a terminal of the
            // same width wraps them again and keeps the wrap marks.
            unwrap: true,
            trim: false,
            extra: sys::GhosttyFormatterTerminalExtra {
                size: size_of::<sys::GhosttyFormatterTerminalExtra>(),
                palette: false,
                modes: extras,
                scrolling_region: extras,
                tabstops: extras,
                pwd: extras,
                keyboard: extras,
                screen: sys::GhosttyFormatterScreenExtra {
                    size: size_of::<sys::GhosttyFormatterScreenExtra>(),
                    cursor: extras,
                    style: extras,
                    hyperlink: extras,
                    protection: extras,
                    kitty_keyboard: extras,
                    charsets: extras,
                },
            },
            selection: ptr::from_ref(selection),
        };
        let mut formatter = ptr::null_mut();
        // SAFETY: the handle is live, `options` is fully initialised and its
        // selection outlives the formatter, which is freed below.
        let result = unsafe {
            sys::ghostty_formatter_terminal_new(ptr::null(), &raw mut formatter, self.0, options)
        };
        if !ok(result) || formatter.is_null() {
            return Vec::new();
        }
        let mut out = ptr::null_mut();
        let mut len = 0usize;
        // SAFETY: `formatter` was just created and both out-pointers are
        // writable.
        let result = unsafe {
            sys::ghostty_formatter_format_alloc(formatter, ptr::null(), &raw mut out, &raw mut len)
        };
        // SAFETY: `formatter` is live and not used again.
        unsafe { sys::ghostty_formatter_free(formatter) };
        if ok(result) {
            take_alloc(out, len)
        } else {
            Vec::new()
        }
    }

    /// An independent copy of the terminal, made through a snapshot.
    pub(crate) fn snapshot_copy(self) -> Option<OwnedTerm> {
        let mut out = ptr::null_mut();
        let mut len = 0usize;
        // SAFETY: the handle is live and both out-pointers are writable.
        let result = unsafe {
            sys::ghostty_snapshot_encode_alloc(self.0, ptr::null(), &raw mut out, &raw mut len)
        };
        if !ok(result) {
            return None;
        }
        let bytes = take_alloc(out, len);
        let mut decoder = ptr::null_mut();
        // SAFETY: the pointer/length describe `bytes`, which outlives the
        // decoder (freed below).
        let result = unsafe {
            sys::ghostty_snapshot_decoder_new_buf(
                ptr::null(),
                &raw mut decoder,
                bytes.as_ptr(),
                bytes.len(),
            )
        };
        if !ok(result) || decoder.is_null() {
            return None;
        }
        let mut copy = ptr::null_mut();
        // SAFETY: `decoder` was just created and `copy` is writable.
        let result = unsafe { sys::ghostty_snapshot_decoder_decode(decoder, &raw mut copy) };
        // SAFETY: `decoder` is live and not used again.
        unsafe { sys::ghostty_snapshot_decoder_free(decoder) };
        (ok(result) && !copy.is_null()).then_some(OwnedTerm(Self(copy)))
    }
}

/// A terminal this value owns and frees.
pub(crate) struct OwnedTerm(pub(crate) Term);

impl Drop for OwnedTerm {
    fn drop(&mut self) {
        // SAFETY: the handle is live, owned by this value alone, and not used
        // after this.
        unsafe { sys::ghostty_terminal_free(self.0.0) };
    }
}

/// Copies and frees a buffer a libghostty-vt `*_alloc` call returned.
pub(crate) fn take_alloc(ptr: *mut u8, len: usize) -> Vec<u8> {
    if ptr.is_null() {
        return Vec::new();
    }
    // SAFETY: the callers pass the pointer/length pair a successful `*_alloc`
    // call just wrote, which describes an initialised allocation.
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len) }.to_vec();
    // SAFETY: the buffer came from the default allocator (NULL) with this
    // length and is not used again.
    unsafe { sys::ghostty_free(ptr::null(), ptr, len) };
    bytes
}

/// Runs an encoder that writes into a caller buffer and reports the needed
/// size with `GHOSTTY_OUT_OF_SPACE`, growing once if the first try is short.
pub(crate) fn encode(
    capacity: usize,
    mut call: impl FnMut(*mut c_char, usize, &mut usize) -> sys::GhosttyResult,
) -> Vec<u8> {
    let mut buf = vec![0u8; capacity];
    let mut len = 0usize;
    let mut result = call(buf.as_mut_ptr().cast(), buf.len(), &mut len);
    if result == sys::GHOSTTY_OUT_OF_SPACE && len > buf.len() {
        buf.resize(len, 0);
        result = call(buf.as_mut_ptr().cast(), buf.len(), &mut len);
    }
    if ok(result) && len <= buf.len() {
        buf.truncate(len);
        buf
    } else {
        Vec::new()
    }
}

/// Reads a render-state value into `out`.
pub(crate) fn render_get<T>(
    state: sys::GhosttyRenderState,
    key: RenderData<T>,
    out: &mut T,
) -> bool {
    // SAFETY: `state` is live (module invariant) and `out` is a valid `T`,
    // the type the header documents for this key.
    ok(unsafe { sys::ghostty_render_state_get(state, key.0, ptr::from_mut(out).cast()) })
}

/// Reads a value of the row the iterator is on.
pub(crate) fn row_get<T>(
    iterator: sys::GhosttyRenderStateRowIterator,
    key: RowData<T>,
    out: &mut T,
) -> bool {
    // SAFETY: `iterator` is live, positioned on a row by a successful
    // `next`, and `out` is a valid `T`, the type documented for this key.
    ok(unsafe { sys::ghostty_render_state_row_get(iterator, key.0, ptr::from_mut(out).cast()) })
}

/// Reads a value of the cell the cell cursor is on.
pub(crate) fn cells_get<T>(
    cells: sys::GhosttyRenderStateRowCells,
    key: CellsData<T>,
    out: &mut T,
) -> bool {
    // SAFETY: `cells` is live, positioned on a cell by a successful `next`,
    // and `out` is a valid `T`, the type documented for this key.
    ok(unsafe { sys::ghostty_render_state_row_cells_get(cells, key.0, ptr::from_mut(out).cast()) })
}

/// Reads the current cell's codepoints into `out`, base first.
pub(crate) fn cells_graphemes(cells: sys::GhosttyRenderStateRowCells, out: &mut Vec<u32>) {
    out.clear();
    let mut len = 0u32;
    if !cells_get(cells, C_GRAPHEMES_LEN, &mut len) || len == 0 {
        return;
    }
    out.resize(len as usize, 0);
    // SAFETY: `cells` is live and positioned; the buffer holds `len` `u32`s,
    // the size GRAPHEMES_LEN just reported for this same cell.
    let result = unsafe {
        sys::ghostty_render_state_row_cells_get(
            cells,
            sys::GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_GRAPHEMES_BUF,
            out.as_mut_ptr().cast(),
        )
    };
    if !ok(result) {
        out.clear();
    }
}

/// Reads a field of a raw cell value.
pub(crate) fn raw_cell<T: Default>(cell: sys::GhosttyCell, key: RawCellData<T>) -> T {
    let mut out = T::default();
    // SAFETY: a raw cell is a plain 64-bit value decoded by the library, and
    // `out` is a valid `T`, the type documented for this key. On failure
    // `out` keeps its default.
    unsafe { sys::ghostty_cell_get(cell, key.0, (&raw mut out).cast()) };
    out
}

/// Reads a field of a raw row value.
pub(crate) fn raw_row<T: Default>(row: sys::GhosttyRow, key: RawRowData<T>) -> T {
    let mut out = T::default();
    // SAFETY: a raw row is a plain 64-bit value decoded by the library, and
    // `out` is a valid `T`, the type documented for this key. On failure
    // `out` keeps its default.
    unsafe { sys::ghostty_row_get(row, key.0, (&raw mut out).cast()) };
    out
}

/// Reads a search value into `out`.
pub(crate) fn search_get<T>(
    search: sys::GhosttySearch,
    key: SearchData<T>,
    out: &mut T,
) -> sys::GhosttyResult {
    // SAFETY: `search` is live and `out` is a valid `T`, the type documented
    // for this key.
    unsafe { sys::ghostty_search_get(search, key.0, ptr::from_mut(out).cast()) }
}

/// Sets an option on a selection-gesture event.
pub(crate) fn gesture_set<T>(
    event: sys::GhosttySelectionGestureEvent,
    opt: GestureOpt<T>,
    value: &T,
) {
    // SAFETY: `event` is live and `value` points to a `T`, the type documented
    // for this option; the event copies it before returning.
    unsafe {
        sys::ghostty_selection_gesture_event_set(event, opt.0, ptr::from_ref(value).cast());
    }
}

/// The raw cell a grid ref points at.
pub(crate) fn grid_ref_cell(grid_ref: &sys::GhosttyGridRef) -> Option<sys::GhosttyCell> {
    let mut cell: sys::GhosttyCell = 0;
    // SAFETY: the callers pass a ref taken from a live terminal that has not
    // been mutated since; `cell` is writable.
    let result = unsafe { sys::ghostty_grid_ref_cell(grid_ref, &raw mut cell) };
    ok(result).then_some(cell)
}

/// The OSC 8 target of the cell a grid ref points at.
pub(crate) fn grid_ref_link(grid_ref: &sys::GhosttyGridRef) -> Option<String> {
    let mut buf = vec![0u8; 512];
    let mut len = 0usize;
    for _ in 0..2 {
        // SAFETY: the callers pass a ref taken from a live terminal that has
        // not been mutated since; the pointer/length describe `buf`.
        let result = unsafe {
            sys::ghostty_grid_ref_hyperlink_uri(grid_ref, buf.as_mut_ptr(), buf.len(), &raw mut len)
        };
        if result == sys::GHOSTTY_OUT_OF_SPACE && len > buf.len() {
            buf.resize(len, 0);
            continue;
        }
        if !ok(result) || len == 0 || len > buf.len() {
            return None;
        }
        buf.truncate(len);
        return Some(String::from_utf8_lossy(&buf).into_owned());
    }
    None
}

impl Term {
    /// Moves the viewport.
    pub(crate) fn scroll_viewport(
        self,
        tag: sys::GhosttyTerminalScrollViewportTag,
        value: sys::GhosttyTerminalScrollViewportValue,
    ) {
        let behavior = sys::GhosttyTerminalScrollViewport { tag, value };
        // SAFETY: the handle is live and `behavior` pairs a tag with the
        // union member the header documents for it (the callers' duty, and
        // every member is a plain integer, so a mismatch is not UB either).
        unsafe { sys::ghostty_terminal_scroll_viewport(self.0, behavior) };
    }

    /// Resizes the grid, reflowing soft-wrapped lines.
    pub(crate) fn resize(self, cols: u16, rows: u16, cell_w: u32, cell_h: u32) -> bool {
        // SAFETY: the handle is live.
        ok(unsafe { sys::ghostty_terminal_resize(self.0, cols, rows, cell_w, cell_h) })
    }

    /// The selection covering the whole screen and scrollback.
    pub(crate) fn all(self) -> Option<sys::GhosttySelection> {
        let mut out = sized_selection();
        // SAFETY: the handle is live and `out` is a sized, writable selection.
        let result = unsafe { sys::ghostty_terminal_select_all(self.0, &raw mut out) };
        ok(result).then_some(out)
    }

    /// The terminal's current selection.
    pub(crate) fn selection(self) -> Option<sys::GhosttySelection> {
        let mut out = sized_selection();
        ok(self.get_into(T_SELECTION, &mut out)).then_some(out)
    }
}

/// An empty selection with its `size` field set, ready to be written into.
pub(crate) fn sized_selection() -> sys::GhosttySelection {
    sys::GhosttySelection {
        size: size_of::<sys::GhosttySelection>(),
        start: sys::GhosttyGridRef {
            size: size_of::<sys::GhosttyGridRef>(),
            ..Default::default()
        },
        end: sys::GhosttyGridRef {
            size: size_of::<sys::GhosttyGridRef>(),
            ..Default::default()
        },
        rectangle: false,
    }
}

/// Copies a string the library lent for the duration of a callback.
///
/// # Safety
///
/// `value` must be NULL/empty or describe `len` readable bytes.
pub(crate) unsafe fn lent_string(value: sys::GhosttyString) -> String {
    if value.ptr.is_null() || value.len == 0 {
        return String::new();
    }
    // SAFETY: the caller guarantees the pointer/length pair is readable.
    let bytes = unsafe { std::slice::from_raw_parts(value.ptr, value.len) };
    String::from_utf8_lossy(bytes).into_owned()
}

/// Resolves a style color against the palette. `None` is "no color set".
pub(crate) fn style_color(
    color: &sys::GhosttyStyleColor,
    palette: &[sys::GhosttyColorRgb; 256],
) -> Option<Rgb> {
    if color.tag == sys::GHOSTTY_STYLE_COLOR_PALETTE {
        // SAFETY: the tag says the union holds a palette index, and any bit
        // pattern is a valid `u8`.
        let index = unsafe { color.value.palette };
        Some(rgb(palette[usize::from(index)]))
    } else if color.tag == sys::GHOSTTY_STYLE_COLOR_RGB {
        // SAFETY: the tag says the union holds an RGB triple, and any bit
        // pattern is a valid one.
        Some(rgb(unsafe { color.value.rgb }))
    } else {
        None
    }
}

/// Brings the render state up to date with the terminal.
pub(crate) fn render_update(state: sys::GhosttyRenderState, term: Term) {
    // SAFETY: both handles are live.
    unsafe { sys::ghostty_render_state_update(state, term.0) };
}

/// Clears the render state's dirty flags.
pub(crate) fn render_clean(state: sys::GhosttyRenderState) {
    // SAFETY: the handle is live.
    unsafe { sys::ghostty_render_state_clean(state) };
}

/// Advances the row iterator; `false` past the last row.
pub(crate) fn row_next(iterator: sys::GhosttyRenderStateRowIterator) -> bool {
    // SAFETY: the handle is live and was bound to the render state by a
    // `R_ROW_ITERATOR` read since that state's last update.
    unsafe { sys::ghostty_render_state_row_iterator_next(iterator) }
}

/// Advances the cell cursor; `false` past the last cell.
pub(crate) fn cells_next(cells: sys::GhosttyRenderStateRowCells) -> bool {
    // SAFETY: the handle is live and was bound to the current row by a
    // `ROW_CELLS` read.
    unsafe { sys::ghostty_render_state_row_cells_next(cells) }
}

/// Clears an option on a selection-gesture event.
pub(crate) fn gesture_clear<T>(event: sys::GhosttySelectionGestureEvent, opt: GestureOpt<T>) {
    // SAFETY: `event` is live; NULL clears the option.
    unsafe { sys::ghostty_selection_gesture_event_set(event, opt.0, ptr::null()) };
}

/// Applies a gesture event; the selection it produces, if it produces one.
pub(crate) fn gesture_apply(
    gesture: sys::GhosttySelectionGesture,
    term: Term,
    event: sys::GhosttySelectionGestureEvent,
) -> Option<sys::GhosttySelection> {
    let mut out = sized_selection();
    // SAFETY: all three handles are live, the event's grid ref (if set) was
    // taken from `term` since its last mutation, and `out` is writable.
    let result =
        unsafe { sys::ghostty_selection_gesture_event(gesture, term.0, event, &raw mut out) };
    ok(result).then_some(out)
}

/// Sets a search option; `None` passes NULL.
pub(crate) fn search_set<T>(
    search: sys::GhosttySearch,
    option: sys::GhosttySearchOption,
    value: Option<&T>,
) -> bool {
    let value = value.map_or(ptr::null(), |value| ptr::from_ref(value).cast::<c_void>());
    // SAFETY: `search` and the terminal it was created with are live (both
    // owned by the same `Vt`), and `value` is NULL or the type the header
    // documents for the option; the search copies it.
    ok(unsafe { sys::ghostty_search_set(search, option, value) })
}

/// Lets the search copy in terminal changes.
pub(crate) fn search_feed(search: sys::GhosttySearch) {
    // SAFETY: `search` and its terminal are live, and the caller has
    // exclusive access to the terminal (`&mut Vt`).
    unsafe { sys::ghostty_search_feed(search) };
}

/// Makes a bounded amount of progress; the status afterwards.
pub(crate) fn search_tick(search: sys::GhosttySearch) -> sys::GhosttySearchStatus {
    let mut status = sys::GHOSTTY_SEARCH_STATUS_COMPLETE;
    // SAFETY: `search` is live and `status` is writable.
    unsafe { sys::ghostty_search_tick(search, &raw mut status) };
    status
}
