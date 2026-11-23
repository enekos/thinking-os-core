//! What a [`crate::Vt`] is configured with and what it reports back: colors,
//! options, events and the frame a renderer draws.

use std::fmt;
use std::sync::Arc;

/// A 24-bit color.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Rgb {
    /// Red.
    pub r: u8,
    /// Green.
    pub g: u8,
    /// Blue.
    pub b: u8,
}

impl Rgb {
    /// A color from its three channels.
    #[must_use]
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }
}

/// The theme a terminal resolves colors against.
///
/// Palette entries 16..=255 (the color cube and the gray ramp) keep Ghostty's
/// defaults.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Colors {
    /// Default text color.
    pub foreground: Rgb,
    /// Default background color.
    pub background: Rgb,
    /// Cursor color. `None` leaves the choice to the renderer.
    pub cursor: Option<Rgb>,
    /// Selection background and, optionally, selection text color.
    ///
    /// When set, [`crate::Vt::frame`] writes these into the `bg`/`fg` of
    /// every selected cell. When `None`, selected cells keep their own colors
    /// and the renderer styles [`Attrs::SELECTED`] itself.
    pub selection: Option<(Rgb, Option<Rgb>)>,
    /// The 16 ANSI colors: 8 normal, then 8 bright.
    pub ansi: [Rgb; 16],
}

/// How to create a [`crate::Vt`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Options {
    /// Width in cells. Zero is treated as one.
    pub cols: u16,
    /// Height in cells. Zero is treated as one.
    pub rows: u16,
    /// Scrollback budget in bytes. Zero disables scrollback. Ghostty prunes
    /// by page (about 400 KB), so the real limit is within one page of this.
    pub scrollback_bytes: usize,
    /// Theme. `None` keeps Ghostty's built-in colors.
    pub colors: Option<Colors>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            cols: 80,
            rows: 24,
            scrollback_bytes: 10 * 1024 * 1024,
            colors: None,
        }
    }
}

/// Why a [`crate::Vt`] could not be created.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// libghostty-vt could not allocate.
    OutOfMemory,
    /// libghostty-vt refused a call; the payload is its `GhosttyResult`.
    Ghostty(i32),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OutOfMemory => f.write_str("libghostty-vt is out of memory"),
            Self::Ghostty(code) => write!(f, "libghostty-vt call failed with result {code}"),
        }
    }
}

impl std::error::Error for Error {}

/// Something the running program asked the terminal's host to do or know.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    /// The title changed (OSC 0/2). Carries the new title, possibly empty.
    Title(String),
    /// BEL.
    Bell,
    /// The working directory changed (OSC 7). Carries what the program sent,
    /// normally a `file://host/path` URL.
    Pwd(String),
    /// The program wrote text to the clipboard (OSC 52 and friends). Writes
    /// over 1 MiB are refused and never reported.
    Clipboard(String),
    /// The program reported its status (OSC 7501).
    Status(ProgramStatus),
}

/// A program status report (OSC 7501).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProgramStatus {
    /// What the program is doing.
    pub state: StatusState,
    /// Why it is blocked, if it is.
    pub kind: StatusKind,
    /// Percent complete, when the program reports one.
    pub progress: Option<u8>,
    /// The program's own id for the thing it reports on.
    pub id: String,
    /// The reporting program's name.
    pub app: String,
    /// Short title.
    pub title: String,
    /// Longer message.
    pub message: String,
}

/// The state in a [`ProgramStatus`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StatusState {
    /// Waiting for input.
    Idle,
    /// Busy.
    Working,
    /// Finished.
    Done,
    /// Waiting on the user; see [`StatusKind`].
    Blocked,
    /// Failed.
    Error,
    /// Forget any earlier report.
    Clear,
}

/// What a blocked program is waiting for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StatusKind {
    /// Nothing in particular.
    None,
    /// A permission prompt.
    Permission,
    /// A question to the user.
    Question,
    /// Authentication.
    Auth,
}

/// The text of one cell: nothing, one `char`, or (rarely) a grapheme cluster
/// of several. Only clusters allocate.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Grapheme(Repr);

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
enum Repr {
    #[default]
    Empty,
    One(char),
    Many(Box<str>),
}

impl Grapheme {
    /// The text of a blank cell.
    pub const EMPTY: Self = Self(Repr::Empty);

    /// A single-codepoint cell.
    #[must_use]
    pub const fn from_char(c: char) -> Self {
        Self(Repr::One(c))
    }

    /// A cell from its codepoints, base first.
    pub fn from_chars(chars: impl IntoIterator<Item = char>) -> Self {
        let mut chars = chars.into_iter();
        let Some(first) = chars.next() else {
            return Self::EMPTY;
        };
        let Some(second) = chars.next() else {
            return Self::from_char(first);
        };
        let mut text = String::new();
        text.push(first);
        text.push(second);
        text.extend(chars);
        Self(Repr::Many(text.into_boxed_str()))
    }

    /// Whether the cell has no text.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        matches!(self.0, Repr::Empty)
    }

    /// The base codepoint, if any.
    #[must_use]
    pub fn first(&self) -> Option<char> {
        match &self.0 {
            Repr::Empty => None,
            Repr::One(c) => Some(*c),
            Repr::Many(text) => text.chars().next(),
        }
    }

    /// The cluster as a string when it has more than one codepoint.
    #[must_use]
    pub fn cluster(&self) -> Option<&str> {
        match &self.0 {
            Repr::Many(text) => Some(text),
            Repr::Empty | Repr::One(_) => None,
        }
    }

    /// Appends the cell's text to `out`.
    pub fn push_to(&self, out: &mut String) {
        match &self.0 {
            Repr::Empty => {}
            Repr::One(c) => out.push(*c),
            Repr::Many(text) => out.push_str(text),
        }
    }
}

impl fmt::Display for Grapheme {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.0 {
            Repr::Empty => Ok(()),
            Repr::One(c) => fmt::Write::write_char(f, *c),
            Repr::Many(text) => f.write_str(text),
        }
    }
}

/// Per-cell flags.
#[derive(Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Attrs(u16);

impl Attrs {
    /// No flags.
    pub const NONE: Self = Self(0);
    /// Bold.
    pub const BOLD: Self = Self(1 << 0);
    /// Italic.
    pub const ITALIC: Self = Self(1 << 1);
    /// Faint (dim).
    pub const FAINT: Self = Self(1 << 2);
    /// Struck through.
    pub const STRIKE: Self = Self(1 << 3);
    /// Invisible: draw the background only.
    pub const INVISIBLE: Self = Self(1 << 4);
    /// The cell holds a double-width character and spans two columns.
    pub const WIDE: Self = Self(1 << 5);
    /// The cell is the second column of a wide character (or the padding a
    /// wide character left at the end of a line when it wrapped). It has no
    /// text of its own.
    pub const SPACER: Self = Self(1 << 6);
    /// The cell is inside the selection.
    pub const SELECTED: Self = Self(1 << 7);
    /// The cell is part of an OSC 8 hyperlink; see [`crate::Vt::link_at`].
    pub const LINK: Self = Self(1 << 8);
    /// Blinking text.
    pub const BLINK: Self = Self(1 << 9);
    /// Overlined.
    pub const OVERLINE: Self = Self(1 << 10);

    /// The raw bits.
    #[must_use]
    pub const fn bits(self) -> u16 {
        self.0
    }

    /// Whether every flag in `other` is set.
    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// Sets the flags in `other`.
    pub const fn insert(&mut self, other: Self) {
        self.0 |= other.0;
    }

    /// Whether no flag is set.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
}

impl std::ops::BitOr for Attrs {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

impl fmt::Debug for Attrs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        const NAMES: [(Attrs, &str); 11] = [
            (Attrs::BOLD, "BOLD"),
            (Attrs::ITALIC, "ITALIC"),
            (Attrs::FAINT, "FAINT"),
            (Attrs::STRIKE, "STRIKE"),
            (Attrs::INVISIBLE, "INVISIBLE"),
            (Attrs::WIDE, "WIDE"),
            (Attrs::SPACER, "SPACER"),
            (Attrs::SELECTED, "SELECTED"),
            (Attrs::LINK, "LINK"),
            (Attrs::BLINK, "BLINK"),
            (Attrs::OVERLINE, "OVERLINE"),
        ];
        let mut list = f.debug_set();
        for (flag, name) in NAMES {
            if self.contains(flag) {
                list.entry(&format_args!("{name}"));
            }
        }
        list.finish()
    }
}

/// Underline style.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Underline {
    /// No underline.
    #[default]
    None,
    /// One line.
    Single,
    /// Two lines.
    Double,
    /// A wavy line.
    Curly,
    /// Dots.
    Dotted,
    /// Dashes.
    Dashed,
}

/// One cell of a [`Line`], ready to draw.
///
/// Reverse video is already applied: `fg` and `bg` are what to paint. `None`
/// means the frame's default ([`Frame::foreground`], [`Frame::background`]).
/// Selected cells carry [`Attrs::SELECTED`]; if [`Colors::selection`] is set
/// their `bg` (and `fg`) are replaced with it as well.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Cell {
    /// The cell's text.
    pub text: Grapheme,
    /// Text color.
    pub fg: Option<Rgb>,
    /// Background color.
    pub bg: Option<Rgb>,
    /// Underline color; `None` means the text color.
    pub underline_color: Option<Rgb>,
    /// Flags.
    pub attrs: Attrs,
    /// Underline style.
    pub underline: Underline,
}

/// One row of the viewport. `cells` always has exactly [`Frame::cols`]
/// entries, blanks included.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Line {
    /// The row's cells, left to right.
    pub cells: Vec<Cell>,
    /// Whether the row soft-wraps into the next one.
    pub wrapped: bool,
}

/// How to draw the cursor.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CursorStyle {
    /// A filled cell.
    Block,
    /// A vertical bar at the cell's left edge.
    Bar,
    /// A line under the cell.
    Underline,
    /// An outlined cell.
    HollowBlock,
}

/// The cursor, present in a [`Frame`] only while visible and in the viewport.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cursor {
    /// Column. On a wide character this is the character's first column.
    pub x: u16,
    /// Viewport row.
    pub y: u16,
    /// Shape (DECSCUSR).
    pub style: CursorStyle,
    /// Whether the program asked for a blinking cursor.
    pub blinking: bool,
    /// Explicit cursor color, from the theme or OSC 12.
    pub color: Option<Rgb>,
}

/// Where the viewport sits in the scrollback, in rows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Scrollbar {
    /// Scrollback plus screen.
    pub total: u64,
    /// The viewport's first row, counted from the oldest row.
    pub offset: u64,
    /// Rows in the viewport.
    pub len: u64,
}

/// Everything a renderer needs for one paint.
///
/// A row that did not change since the previous frame is the same `Arc`
/// (`Arc::ptr_eq`), so per-row caches can key on the pointer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    /// Width in cells.
    pub cols: u16,
    /// Height in cells; `lines.len()`.
    pub rows: u16,
    /// The viewport, top to bottom.
    pub lines: Vec<Arc<Line>>,
    /// The cursor, if it should be drawn.
    pub cursor: Option<Cursor>,
    /// Default text color (theme, or OSC 10).
    pub foreground: Rgb,
    /// Default background color (theme, or OSC 11).
    pub background: Rgb,
    /// Scroll position.
    pub scrollbar: Scrollbar,
    /// Whether the viewport follows the live screen (not scrolled back).
    pub at_bottom: bool,
}
