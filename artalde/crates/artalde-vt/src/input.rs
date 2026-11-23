//! What a host feeds a [`crate::Vt`]: keys, mouse, scrolling, selection
//! gestures and search.

use std::fmt;

/// Keyboard modifiers.
#[derive(Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Mods(u8);

impl Mods {
    /// No modifier.
    pub const NONE: Self = Self(0);
    /// Shift.
    pub const SHIFT: Self = Self(1 << 0);
    /// Control.
    pub const CTRL: Self = Self(1 << 1);
    /// Alt (Option on macOS).
    pub const ALT: Self = Self(1 << 2);
    /// Super (Command on macOS).
    pub const SUPER: Self = Self(1 << 3);

    /// Whether every modifier in `other` is held.
    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// Whether no modifier is held.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
}

impl std::ops::BitOr for Mods {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

impl std::ops::BitOrAssign for Mods {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

impl fmt::Debug for Mods {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut list = f.debug_set();
        for (flag, name) in [
            (Self::SHIFT, "SHIFT"),
            (Self::CTRL, "CTRL"),
            (Self::ALT, "ALT"),
            (Self::SUPER, "SUPER"),
        ] {
            if self.contains(flag) {
                list.entry(&format_args!("{name}"));
            }
        }
        list.finish()
    }
}

/// A physical key, named by its US-layout position.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[allow(
    missing_docs,
    reason = "the variant names are the documentation: one per key"
)]
pub enum Key {
    A,
    B,
    C,
    D,
    E,
    F,
    G,
    H,
    I,
    J,
    K,
    L,
    M,
    N,
    O,
    P,
    Q,
    R,
    S,
    T,
    U,
    V,
    W,
    X,
    Y,
    Z,
    Digit0,
    Digit1,
    Digit2,
    Digit3,
    Digit4,
    Digit5,
    Digit6,
    Digit7,
    Digit8,
    Digit9,
    Backquote,
    Backslash,
    BracketLeft,
    BracketRight,
    Comma,
    Equal,
    Minus,
    Period,
    Quote,
    Semicolon,
    Slash,
    Enter,
    Tab,
    Escape,
    Backspace,
    Space,
    Delete,
    Insert,
    Home,
    End,
    PageUp,
    PageDown,
    ArrowUp,
    ArrowDown,
    ArrowLeft,
    ArrowRight,
    F1,
    F2,
    F3,
    F4,
    F5,
    F6,
    F7,
    F8,
    F9,
    F10,
    F11,
    F12,
    Numpad0,
    Numpad1,
    Numpad2,
    Numpad3,
    Numpad4,
    Numpad5,
    Numpad6,
    Numpad7,
    Numpad8,
    Numpad9,
    NumpadAdd,
    NumpadSubtract,
    NumpadMultiply,
    NumpadDivide,
    NumpadDecimal,
    NumpadEnter,
    NumpadEqual,
    /// A key with no name here. Its `text` still encodes.
    Unidentified,
}

/// What happened to a key.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum KeyAction {
    /// Went down.
    #[default]
    Press,
    /// Auto-repeated while held.
    Repeat,
    /// Went up. Only encodes under the kitty keyboard protocol.
    Release,
}

/// One key event to encode.
#[derive(Clone, Copy, Debug)]
pub struct KeyInput<'a> {
    /// The physical key.
    pub key: Key,
    /// Modifiers held.
    pub mods: Mods,
    /// Modifiers the layout used up to produce `text` (Shift for `!`).
    /// Ignored when `text` is `None`.
    pub consumed: Mods,
    /// The text the key produced, if it is printable. Control characters and
    /// the private-use codes macOS sends for function keys are dropped.
    pub text: Option<&'a str>,
    /// The character the key produces without Shift, if known.
    pub unshifted: Option<char>,
    /// Press, repeat or release.
    pub action: KeyAction,
    /// macOS: treat Option as Alt (send ESC-prefixed keys) instead of as a
    /// compose key.
    pub option_as_alt: bool,
}

impl KeyInput<'_> {
    /// A plain press of `key` with nothing else set.
    #[must_use]
    pub const fn press(key: Key) -> Self {
        Self {
            key,
            mods: Mods::NONE,
            consumed: Mods::NONE,
            text: None,
            unshifted: None,
            action: KeyAction::Press,
            option_as_alt: false,
        }
    }
}

/// Where the cell grid sits inside the widget, in pixels. Cell sizes may be
/// fractional.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Geometry {
    /// Cell width.
    pub cell_w: f32,
    /// Cell height.
    pub cell_h: f32,
    /// Space left of the first column.
    pub pad_x: f32,
    /// Space above the first row.
    pub pad_y: f32,
}

/// What the pointer did.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MouseAction {
    /// A button went down (or the wheel turned one notch).
    Press,
    /// A button went up.
    Release,
    /// The pointer moved.
    Motion,
}

/// A mouse button. The wheel is four buttons, as terminals report it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MouseButton {
    /// Primary.
    Left,
    /// Secondary.
    Right,
    /// Middle.
    Middle,
    /// Wheel away from the user.
    WheelUp,
    /// Wheel toward the user.
    WheelDown,
    /// Wheel left.
    WheelLeft,
    /// Wheel right.
    WheelRight,
}

impl MouseButton {
    /// Whether this is one of the wheel directions.
    #[must_use]
    pub const fn is_wheel(self) -> bool {
        matches!(
            self,
            Self::WheelUp | Self::WheelDown | Self::WheelLeft | Self::WheelRight
        )
    }
}

/// One mouse event to encode.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MouseInput {
    /// Press, release or motion.
    pub action: MouseAction,
    /// The button pressed or released; for motion, the button held, if any.
    pub button: Option<MouseButton>,
    /// Modifiers held.
    pub mods: Mods,
    /// Pointer x in pixels from the widget's left edge (padding included).
    pub x: f32,
    /// Pointer y in pixels from the widget's top edge (padding included).
    pub y: f32,
    /// Where the grid is.
    pub geometry: Geometry,
}

/// A viewport scroll.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Scroll {
    /// By rows: negative toward older output, positive toward the live screen.
    Lines(isize),
    /// By screenfuls, same signs.
    Pages(isize),
    /// To the oldest row.
    Top,
    /// Back to the live screen.
    Bottom,
    /// So that this row (counted from the oldest, like
    /// [`crate::Scrollbar::offset`]) is first in the viewport.
    ToRow(usize),
}

/// A step of a mouse selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SelectStep {
    /// Button down: starts a selection. A second and third press in the same
    /// place within 500 ms select the word, then the line.
    Press,
    /// Pointer moved with the button down: extends it.
    Drag,
    /// Button up.
    Release,
}

/// Where a selection step happened.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SelectPoint {
    /// Viewport column under the pointer (clamped to the grid).
    pub col: u16,
    /// Viewport row under the pointer (clamped to the grid).
    pub row: u16,
    /// Pointer x in pixels from the widget's left edge (padding included).
    /// Decides which half of a cell a drag has reached.
    pub x: f32,
    /// Pointer y in pixels from the widget's top edge (padding included).
    pub y: f32,
    /// Where the grid is.
    pub geometry: Geometry,
    /// Select a rectangle instead of flowing text (Alt-drag).
    pub rectangle: bool,
}

/// Progress of a search.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct SearchStatus {
    /// Matches found so far.
    pub total: usize,
    /// The selected match, 0 being the newest (nearest the bottom). Show it
    /// as `selected + 1` of `total`.
    pub selected: Option<usize>,
    /// Whether the whole scrollback has been searched. While `false`, call
    /// [`crate::Vt::search_tick`] again.
    pub complete: bool,
}

/// A stretch of one viewport row covered by a search match.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SearchMatch {
    /// Viewport row.
    pub row: u16,
    /// First column.
    pub col_start: u16,
    /// One past the last column.
    pub col_end: u16,
    /// Whether this is the selected match.
    pub selected: bool,
}
