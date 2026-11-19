//! Commands: what keys, the palette, buttons and mods ask the app to do.
//! Written `name` or `name:arg`, e.g. `layout.select:focus`.

use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Command {
    pub name: String,
    pub arg: Option<String>,
}

impl Command {
    pub fn new(name: &str, arg: Option<&str>) -> Command {
        Command {
            name: name.to_owned(),
            arg: arg.map(str::to_owned),
        }
    }

    pub fn parse(s: &str) -> Command {
        match s.split_once(':') {
            Some((name, arg)) => Command::new(name.trim(), Some(arg)),
            None => Command::new(s.trim(), None),
        }
    }
}

impl fmt::Display for Command {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.arg {
            Some(arg) => write!(f, "{}:{arg}", self.name),
            None => f.write_str(&self.name),
        }
    }
}

/// Every command the app understands, with a one-line description, for
/// the palette and `artalde commands`.
pub const COMMANDS: &[(&str, &str)] = &[
    ("palette.open", "Open the command palette"),
    ("agent.next", "Focus the next agent that needs you"),
    ("agent.select", "Select an agent: <session id> or <n>"),
    (
        "session.focus",
        "Bring a session's terminal to the front: <session id>",
    ),
    ("session.ack", "Mark a done session as seen: <session id>"),
    (
        "session.send",
        "Type a line into the selected session's terminal: <text>",
    ),
    (
        "session.key",
        "Press a key in the selected session's terminal: <1|2|3|enter|esc>",
    ),
    (
        "space.filter",
        "Show only one repo's agents: <repo>, empty clears",
    ),
    ("spawn.open", "Start a new agent in a repo or worktree"),
    ("layout.select", "Switch layout: <name>"),
    ("layout.cycle", "Switch to the next layout"),
    ("theme.select", "Switch theme: <name>"),
    ("theme.cycle", "Switch to the next theme"),
    ("inbox.toggle", "Tick or untick a proposal: <index>"),
    ("url.open", "Open a URL: <url>"),
    ("path.open", "Open a folder in Finder: <path>"),
    ("config.open", "Open the config folder"),
    ("refresh", "Probe everything now"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_display() {
        let c = Command::parse("url.open:https://x.y/z");
        assert_eq!(c, Command::new("url.open", Some("https://x.y/z")));
        assert_eq!(c.to_string(), "url.open:https://x.y/z");
        assert_eq!(Command::parse("refresh").arg, None);
    }
}
