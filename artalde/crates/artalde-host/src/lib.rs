//! The terminal host: PTYs, daemon, wire protocol and client. See `docs/terminals.md`.
//!
//! - [`Host`] owns the terminals and serves them on a unix socket.
//! - [`Client`] talks to it; [`Stream`] is one attached terminal, [`Events`]
//!   the change feed.
//! - [`Screen`] is what the host knows about a terminal's display.
//!
//! # Keeping a model in sync
//!
//! Call [`Client::subscribe`] first, then [`Client::list`], then apply events
//! as they come. Events carry absolute values, so one that was already in the
//! list changes nothing, and a stale one is followed by every later one for
//! the same terminal: the model converges on the host's state.

mod client;
mod env;
mod host;
mod paths;
mod procinfo;
mod pty;
mod queue;
mod screen;
mod sys;
mod tty;
mod util;
pub mod wire;

pub use client::{Client, ClientError, Events, Exit, RunOptions, Spawn, Stream};
pub use host::{Host, HostError, HostHandle};
pub use paths::socket_path;
pub use screen::{Screen, ScreenEvent, StatusReport};
pub use tty::{AttachEnd, attach_tty};
pub use wire::{Event, HostInfo, State, Status, StatusState, TerminalInfo};
