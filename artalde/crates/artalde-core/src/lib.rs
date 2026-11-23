//! artalde's model, independent of any UI.
//!
//! - [`hook`] turns Claude Code hook events into [`model::SessionRecord`]s.
//! - [`store`] keeps those records as one JSON file per session.
//! - [`probe`] reads processes, git, `gh`, transcripts and task lists.
//! - [`inbox`] parses zaindu's proposal files.
//! - [`lifecycle`] holds the spawn → stale → close → reopen rules.
//! - [`config`] loads the live-reloaded layouts, themes, keys and settings.

pub mod catalog;
pub mod changes;
pub mod claude;
pub mod command;
pub mod config;
pub mod github;
pub mod hook;
pub mod inbox;
pub mod keys;
pub mod layout;
pub mod lifecycle;
pub mod model;
pub mod panes;
pub mod probe;
pub mod review;
pub mod store;
pub mod tags;
pub mod text;
pub mod theme;

/// Seconds since the Unix epoch.
pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// `$HOME`, if set.
pub fn home() -> Option<std::path::PathBuf> {
    std::env::var_os("HOME").map(std::path::PathBuf::from)
}
