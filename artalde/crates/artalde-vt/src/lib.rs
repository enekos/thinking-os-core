//! Ghostty's terminal emulator behind a safe type. See `docs/terminals.md`.
//!
//! [`Vt`] is one terminal: screen, scrollback, modes, selection and search,
//! with libghostty-vt (built from the pinned `vendor/ghostty` and linked
//! statically) doing the emulation. It has no I/O of its own. The host reads
//! the pty and calls [`Vt::write`]; whatever must go back to the program
//! comes out of [`Vt::take_replies`] and the input encoders ([`Vt::key`],
//! [`Vt::mouse`], [`Vt::paste`], [`Vt::focus`]); a renderer draws
//! [`Vt::frame`] whenever [`Vt::changed`] says so.
//!
//! ```
//! use artalde_vt::{Options, Vt};
//!
//! let mut vt = Vt::new(&Options::default())?;
//! vt.write(b"hello \x1b[1mworld\x1b[0m\r\n");
//! assert_eq!(vt.text(0), "hello world");
//! # Ok::<(), artalde_vt::Error>(())
//! ```
//!
//! Kitty graphics are not exposed: the protocol is switched off (a zero
//! storage budget), so image sequences are parsed and dropped, and the size
//! callback such programs query first is still answered.

mod ffi;
mod input;
mod sys;
mod types;
mod vt;

pub use input::{
    Geometry, Key, KeyAction, KeyInput, Mods, MouseAction, MouseButton, MouseInput, Scroll,
    SearchMatch, SearchStatus, SelectPoint, SelectStep,
};
pub use types::{
    Attrs, Cell, Colors, Cursor, CursorStyle, Error, Event, Frame, Grapheme, Line, Options,
    ProgramStatus, Rgb, Scrollbar, StatusKind, StatusState, Underline,
};
pub use vt::Vt;
