//! Claude Code hook events → session state.
//!
//! Claude pipes one JSON object per hook on stdin. The state machine:
//!
//! | event                         | state                               |
//! |-------------------------------|-------------------------------------|
//! | `SessionStart`                | idle (if new)                       |
//! | `UserPromptSubmit`            | working                             |
//! | `PreToolUse` / `PostToolUse`  | working                             |
//! | `PermissionRequest`           | blocked                             |
//! | `Notification` (permission)   | blocked                             |
//! | `Notification` (idle prompt)  | done, if it was working             |
//! | `Stop`                        | done                                |
//! | `SessionEnd`                  | record removed                      |

use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::Value;

use crate::model::{AgentState, SessionRecord};
use crate::text::clip;

const TITLE_CHARS: usize = 80;
const PROMPT_CHARS: usize = 240;
const ACTIVITY_CHARS: usize = 72;
const MESSAGE_CHARS: usize = 200;

/// The fields of a hook payload artalde reads; the rest is ignored.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct HookEvent {
    pub hook_event_name: String,
    pub session_id: String,
    pub cwd: Option<PathBuf>,
    pub transcript_path: Option<PathBuf>,
    pub prompt: Option<String>,
    pub message: Option<String>,
    pub notification_type: Option<String>,
    pub tool_name: Option<String>,
    pub tool_input: Value,
}

/// What the caller should do with the record after [`apply`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Keep,
    /// The session ended: delete its record.
    Remove,
}

/// Fold one event into `rec`. `now` is Unix seconds.
pub fn apply(rec: &mut SessionRecord, ev: &HookEvent, now: u64) -> Outcome {
    if rec.id.is_empty() {
        rec.id.clone_from(&ev.session_id);
    }
    if rec.agent.is_empty() {
        rec.agent = "claude".into();
    }
    if let Some(cwd) = &ev.cwd {
        rec.cwd.clone_from(cwd);
    }
    if ev.transcript_path.is_some() {
        rec.transcript.clone_from(&ev.transcript_path);
    }
    rec.updated = now;

    let next = match ev.hook_event_name.as_str() {
        "SessionEnd" => return Outcome::Remove,
        "SessionStart" => (rec.since == 0).then_some(AgentState::Idle),
        "UserPromptSubmit" => {
            if let Some(prompt) = ev.prompt.as_deref().filter(|p| !p.trim().is_empty()) {
                if rec.title.is_none() {
                    rec.title = Some(clip(prompt, TITLE_CHARS));
                }
                rec.last_prompt = Some(clip(prompt, PROMPT_CHARS));
            }
            rec.message = None;
            rec.activity = None;
            Some(AgentState::Working)
        }
        "PreToolUse" => {
            if let Some(tool) = &ev.tool_name {
                rec.activity = Some(describe_tool(tool, &ev.tool_input));
                if let Some(path) = touched_path(tool, &ev.tool_input) {
                    rec.touched = Some(path);
                }
            }
            rec.message = None;
            Some(AgentState::Working)
        }
        "PostToolUse" => {
            rec.message = None;
            Some(AgentState::Working)
        }
        "PermissionRequest" => {
            let what = ev
                .tool_name
                .as_deref()
                .map(|t| describe_tool(t, &ev.tool_input))
                .or_else(|| rec.activity.clone())
                .unwrap_or_else(|| "a tool".into());
            rec.message = Some(clip(&format!("allow {what}?"), MESSAGE_CHARS));
            Some(AgentState::Blocked)
        }
        "Notification" => {
            let msg = ev.message.as_deref().unwrap_or("");
            if is_idle_prompt(ev.notification_type.as_deref(), msg) {
                (rec.state == AgentState::Working || rec.state == AgentState::Unknown)
                    .then_some(AgentState::Done)
            } else {
                rec.message = Some(clip(
                    if msg.is_empty() { "needs you" } else { msg },
                    MESSAGE_CHARS,
                ));
                Some(AgentState::Blocked)
            }
        }
        "Stop" => {
            rec.activity = None;
            rec.message = None;
            Some(AgentState::Done)
        }
        _ => None,
    };
    if let Some(state) = next {
        if state != rec.state || rec.since == 0 {
            rec.since = now;
        }
        rec.state = state;
    }
    Outcome::Keep
}

fn is_idle_prompt(kind: Option<&str>, message: &str) -> bool {
    match kind {
        Some(k) => k == "idle_prompt",
        None => message.contains("waiting for your input"),
    }
}

/// `Bash · cargo test`, `Edit · app.rs`, `Grep · TODO`.
pub fn describe_tool(tool: &str, input: &Value) -> String {
    let field = |k: &str| input.get(k).and_then(Value::as_str);
    let file = |k: &str| {
        field(k).map(|p| {
            Path::new(p)
                .file_name()
                .map_or_else(|| p.to_owned(), |n| n.to_string_lossy().into_owned())
        })
    };
    let detail = match tool {
        "Bash" => field("description").or(field("command")).map(str::to_owned),
        "Edit" | "Write" | "Read" | "MultiEdit" => file("file_path"),
        "NotebookEdit" => file("notebook_path"),
        "Grep" | "Glob" => field("pattern").map(str::to_owned),
        "Task" | "Agent" => field("description").map(str::to_owned),
        "WebFetch" => field("url").map(str::to_owned),
        "WebSearch" => field("query").map(str::to_owned),
        _ => None,
    };
    let name = tool.rsplit("__").next().unwrap_or(tool);
    match detail {
        Some(d) if !d.trim().is_empty() => clip(&format!("{name} · {d}"), ACTIVITY_CHARS),
        _ => clip(name, ACTIVITY_CHARS),
    }
}

/// The absolute path a tool call works on, if it names one: file tools'
/// paths, and `cd <dir>` / `git -C <dir>` / `zz -p`-free Bash prefixes.
pub fn touched_path(tool: &str, input: &Value) -> Option<PathBuf> {
    let field = |k: &str| input.get(k).and_then(Value::as_str);
    let raw = match tool {
        "Edit" | "Write" | "Read" | "MultiEdit" => field("file_path"),
        "NotebookEdit" => field("notebook_path"),
        "Grep" | "Glob" => field("path"),
        "Bash" => field("command").and_then(bash_dir),
        _ => None,
    }?;
    let path = PathBuf::from(raw);
    path.is_absolute().then_some(path)
}

/// The directory a shell command starts by moving into.
fn bash_dir(cmd: &str) -> Option<&str> {
    let mut words = cmd.split_whitespace();
    match words.next()? {
        "cd" => words.next(),
        "git" => (words.next()? == "-C").then(|| words.next()).flatten(),
        _ => None,
    }
    .map(|w| w.trim_matches(|c| c == '"' || c == '\'' || c == ';' || c == '&'))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn ev(name: &str, extra: Value) -> HookEvent {
        let mut v = json!({ "hook_event_name": name, "session_id": "s1", "cwd": "/w" });
        if let (Some(obj), Value::Object(more)) = (v.as_object_mut(), extra) {
            obj.extend(more);
        }
        serde_json::from_value(v).unwrap()
    }

    #[test]
    fn a_turn_goes_working_then_done() {
        let mut rec = SessionRecord::default();
        apply(&mut rec, &ev("SessionStart", json!({})), 10);
        assert_eq!((rec.state, rec.since), (AgentState::Idle, 10));
        apply(
            &mut rec,
            &ev(
                "UserPromptSubmit",
                json!({ "prompt": "fix the login bug\nmore" }),
            ),
            20,
        );
        assert_eq!(rec.state, AgentState::Working);
        assert_eq!(rec.title.as_deref(), Some("fix the login bug"));
        apply(
            &mut rec,
            &ev(
                "PreToolUse",
                json!({ "tool_name": "Edit", "tool_input": { "file_path": "/repo/src/a.rs" } }),
            ),
            25,
        );
        assert_eq!(rec.activity.as_deref(), Some("Edit · a.rs"));
        assert_eq!(rec.touched.as_deref(), Some(Path::new("/repo/src/a.rs")));
        assert_eq!(rec.since, 20, "same state keeps its start");
        apply(&mut rec, &ev("Stop", json!({})), 30);
        assert_eq!(
            (rec.state, rec.since, rec.activity.as_deref()),
            (AgentState::Done, 30, None)
        );
    }

    #[test]
    fn permission_blocks_and_tool_use_unblocks() {
        let mut rec = SessionRecord::default();
        apply(
            &mut rec,
            &ev(
                "PermissionRequest",
                json!({ "tool_name": "Bash", "tool_input": { "command": "rm -rf target" } }),
            ),
            5,
        );
        assert_eq!(rec.state, AgentState::Blocked);
        assert_eq!(rec.message.as_deref(), Some("allow Bash · rm -rf target?"));
        apply(
            &mut rec,
            &ev("PostToolUse", json!({ "tool_name": "Bash" })),
            6,
        );
        assert_eq!(
            (rec.state, rec.message.as_deref()),
            (AgentState::Working, None)
        );
    }

    #[test]
    fn idle_prompt_only_finishes_a_working_turn() {
        let idle = json!({ "notification_type": "idle_prompt", "message": "Claude is waiting for your input" });
        let mut rec = SessionRecord {
            state: AgentState::Idle,
            since: 1,
            ..Default::default()
        };
        apply(&mut rec, &ev("Notification", idle.clone()), 5);
        assert_eq!(rec.state, AgentState::Idle);
        rec.state = AgentState::Working;
        apply(&mut rec, &ev("Notification", idle), 6);
        assert_eq!(rec.state, AgentState::Done);
        apply(
            &mut rec,
            &ev(
                "Notification",
                json!({ "message": "Claude needs your permission to use Bash" }),
            ),
            7,
        );
        assert_eq!(rec.state, AgentState::Blocked);
    }

    #[test]
    fn session_end_removes() {
        let mut rec = SessionRecord::default();
        assert_eq!(
            apply(&mut rec, &ev("SessionEnd", json!({})), 1),
            Outcome::Remove
        );
    }

    #[test]
    fn bash_prefixes_name_the_directory() {
        let bash = |c: &str| touched_path("Bash", &json!({ "command": c }));
        assert_eq!(bash("cd /a/b && cargo test"), Some(PathBuf::from("/a/b")));
        assert_eq!(bash("git -C \"/r\" status"), Some(PathBuf::from("/r")));
        assert_eq!(bash("cd relative"), None);
        assert_eq!(bash("cargo test"), None);
    }

    #[test]
    fn mcp_tools_show_their_short_name() {
        assert_eq!(
            describe_tool("mcp__claude_ai_Linear__get_issue", &json!({})),
            "get_issue"
        );
    }
}
