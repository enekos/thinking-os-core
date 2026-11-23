//! Environment hygiene: the host is often started from inside an agent's
//! shell, and must not pass that session's identity on to every terminal.

use std::ffi::OsString;

const DROP_EXACT: &[&str] = &[
    "ARTALDE_TERMINAL",
    "ARTALDE_HOST_SOCKET",
    "CLAUDECODE",
    "TERM",
    "TERM_PROGRAM",
    "TERM_PROGRAM_VERSION",
    "TERM_SESSION_ID",
    "TERMINFO",
    "ITERM_SESSION_ID",
    "ITERM_PROFILE",
    "COLORTERM",
    "COLORFGBG",
    "LC_TERMINAL",
    "LC_TERMINAL_VERSION",
    "TMUX",
    "TMUX_PANE",
    "STY",
    "WINDOW",
    "WINDOWID",
    "VTE_VERSION",
    "LINES",
    "COLUMNS",
    "SHLVL",
    "OLDPWD",
    "PWD",
    "SSH_CLIENT",
    "SSH_CONNECTION",
    "SSH_TTY",
    "INVOCATION_ID",
    "JOURNAL_STREAM",
    "CURSOR_TRACE_ID",
    "GIT_ASKPASS",
    "AI_AGENT",
];

const DROP_PREFIX: &[&str] = &[
    "CLAUDE_CODE_",
    "CLAUDE_AGENT_",
    "CODEX_SANDBOX",
    "HC_",
    "HERDR_",
    "KITTY_",
    "GHOSTTY_",
    "WEZTERM_",
    "ALACRITTY_",
    "KONSOLE_",
    "ZELLIJ",
    "VSCODE_",
    "npm_",
];

fn dropped(name: &str) -> bool {
    DROP_EXACT.contains(&name) || DROP_PREFIX.iter().any(|p| name.starts_with(p))
}

/// This process's environment without the variables that describe the
/// terminal or agent session it happens to run in.
pub(crate) fn scrubbed() -> Vec<(OsString, OsString)> {
    std::env::vars_os()
        .filter(|(k, _)| k.to_str().is_none_or(|k| !dropped(k)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::dropped;

    #[test]
    fn drops_session_variables_and_keeps_the_rest() {
        for name in [
            "ARTALDE_TERMINAL",
            "CLAUDECODE",
            "CLAUDE_CODE_ENTRYPOINT",
            "TERM_SESSION_ID",
            "ITERM_SESSION_ID",
            "TMUX_PANE",
            "npm_config_prefix",
        ] {
            assert!(dropped(name), "{name}");
        }
        for name in ["PATH", "HOME", "SSH_AUTH_SOCK", "ARTALDE_STATE_DIR", "LANG"] {
            assert!(!dropped(name), "{name}");
        }
    }
}
