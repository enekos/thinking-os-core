//! Read-only probes of the machine: processes, git, `gh`, transcripts and
//! Claude task lists. Each runs a short external command or reads a bounded
//! slice of a file; none of them writes anything.

use std::collections::HashMap;
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::Value;

use crate::model::{Pr, Worktree};
use crate::text::clip_block;

/// How much of a transcript's end is read for the last assistant message.
const TRANSCRIPT_TAIL_BYTES: u64 = 256 * 1024;
const LAST_SAID_CHARS: usize = 600;
const MAX_TASK_FILES: usize = 200;

/// Process names that count as agents.
pub const AGENTS: [&str; 3] = ["claude", "codex", "opencode"];

fn output(cmd: &mut Command) -> Option<String> {
    let out = cmd
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Proc {
    pub pid: u32,
    pub ppid: u32,
    /// `ttys004`, or `None` for `??`.
    pub tty: Option<String>,
    /// Seconds since the process started.
    pub elapsed: u64,
    /// The executable's base name.
    pub name: String,
}

/// Every process, from one `ps` call.
pub fn processes() -> Vec<Proc> {
    output(Command::new("ps").args(["-axo", "pid=,ppid=,etime=,tty=,comm="]))
        .map(|s| parse_ps(&s))
        .unwrap_or_default()
}

pub fn parse_ps(text: &str) -> Vec<Proc> {
    text.lines()
        .filter_map(|line| {
            let mut it = line.split_whitespace();
            let pid = it.next()?.parse().ok()?;
            let ppid = it.next()?.parse().ok()?;
            let elapsed = parse_etime(it.next()?)?;
            let tty = it.next()?;
            let comm = it.collect::<Vec<_>>().join(" ");
            let name = Path::new(&comm).file_name()?.to_string_lossy().into_owned();
            let tty = (tty != "??" && tty != "-").then(|| tty.to_owned());
            Some(Proc {
                pid,
                ppid,
                tty,
                elapsed,
                name,
            })
        })
        .collect()
}

/// `ps` elapsed time, `[[dd-]hh:]mm:ss`, in seconds.
pub fn parse_etime(s: &str) -> Option<u64> {
    let (days, rest) = match s.split_once('-') {
        Some((d, r)) => (d.parse::<u64>().ok()?, r),
        None => (0, s),
    };
    let parts: Vec<u64> = rest
        .split(':')
        .map(|p| p.parse().ok())
        .collect::<Option<_>>()?;
    let (h, m, sec) = match parts.as_slice() {
        [m, s] => (0, *m, *s),
        [h, m, s] => (*h, *m, *s),
        _ => return None,
    };
    Some(days * 86_400 + h * 3600 + m * 60 + sec)
}

/// Interactive agents: agent-named processes on a terminal.
pub fn agent_processes(procs: &[Proc]) -> Vec<&Proc> {
    procs
        .iter()
        .filter(|p| p.tty.is_some() && AGENTS.contains(&p.name.as_str()))
        .collect()
}

/// The nearest agent process at or above `start`.
pub fn agent_ancestor(procs: &[Proc], start: u32) -> Option<&Proc> {
    let by_pid: HashMap<u32, &Proc> = procs.iter().map(|p| (p.pid, p)).collect();
    let mut pid = start;
    for _ in 0..8 {
        let p = by_pid.get(&pid)?;
        if AGENTS.contains(&p.name.as_str()) {
            return Some(p);
        }
        pid = p.ppid;
    }
    None
}

/// Working directories of `pids`, from one `lsof` call.
pub fn cwds(pids: &[u32]) -> HashMap<u32, PathBuf> {
    if pids.is_empty() {
        return HashMap::new();
    }
    let list = pids
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(",");
    let Some(text) = output(Command::new("lsof").args(["-a", "-d", "cwd", "-p", &list, "-Fpn"]))
    else {
        return HashMap::new();
    };
    let mut out = HashMap::new();
    let mut pid = None;
    for line in text.lines() {
        if let Some(p) = line.strip_prefix('p') {
            pid = p.parse().ok();
        } else if let (Some(n), Some(p)) = (line.strip_prefix('n'), pid) {
            out.insert(p, PathBuf::from(n));
        }
    }
    out
}

/// Where a path's checkout starts, and the repository's name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitPlace {
    pub worktree: PathBuf,
    /// The main checkout (parent of the common `.git`).
    pub root: PathBuf,
    pub repo: String,
}

pub fn git_place(path: &Path) -> Option<GitPlace> {
    let dir = if path.is_dir() { path } else { path.parent()? };
    let text = output(Command::new("git").arg("-C").arg(dir).args([
        "rev-parse",
        "--path-format=absolute",
        "--show-toplevel",
        "--git-common-dir",
    ]))?;
    let mut lines = text.lines();
    let worktree = PathBuf::from(lines.next()?);
    let common = PathBuf::from(lines.next()?);
    let root = if common.file_name().is_some_and(|n| n == ".git") {
        common.parent()?.to_path_buf()
    } else {
        common
    };
    let repo = root
        .file_name()?
        .to_string_lossy()
        .trim_end_matches(".git")
        .to_owned();
    Some(GitPlace {
        worktree,
        root,
        repo,
    })
}

/// Branch, dirt and divergence of one worktree.
pub fn worktree_status(path: &Path) -> Option<Worktree> {
    let text = output(Command::new("git").arg("-C").arg(path).args([
        "status",
        "--porcelain=v2",
        "--branch",
        "--untracked-files=normal",
    ]))?;
    Some(parse_status(path, &text))
}

pub fn parse_status(path: &Path, text: &str) -> Worktree {
    let mut wt = Worktree {
        path: path.to_path_buf(),
        ..Default::default()
    };
    for line in text.lines() {
        if let Some(head) = line.strip_prefix("# branch.head ") {
            wt.branch = (head != "(detached)").then(|| head.to_owned());
        } else if let Some(ab) = line.strip_prefix("# branch.ab ") {
            let mut it = ab.split_whitespace();
            wt.ahead = it
                .next()
                .and_then(|a| a.trim_start_matches('+').parse().ok())
                .unwrap_or(0);
            wt.behind = it
                .next()
                .and_then(|b| b.trim_start_matches('-').parse().ok())
                .unwrap_or(0);
        } else if !line.starts_with('#') && !line.is_empty() {
            wt.dirty = wt.dirty.saturating_add(1);
        }
    }
    wt
}

/// The paths of every worktree of the repository at `root`.
pub fn worktree_paths(root: &Path) -> Vec<PathBuf> {
    output(
        Command::new("git")
            .arg("-C")
            .arg(root)
            .args(["worktree", "list", "--porcelain"]),
    )
    .map(|text| {
        text.lines()
            .filter_map(|l| l.strip_prefix("worktree "))
            .map(PathBuf::from)
            .collect()
    })
    .unwrap_or_default()
}

/// `owner/repo` of the checkout's `origin` remote, when it is GitHub.
pub fn remote_slug(root: &Path) -> Option<String> {
    let url = output(
        Command::new("git")
            .arg("-C")
            .arg(root)
            .args(["remote", "get-url", "origin"]),
    )?;
    crate::github::slug_from_remote(&url)
}

/// Open PRs the `gh` user authored.
pub fn my_prs(limit: u32) -> Result<Vec<Pr>, String> {
    let text = output(Command::new("gh").args([
        "search",
        "prs",
        "--author",
        "@me",
        "--state",
        "open",
        "--sort",
        "updated",
        "--limit",
        &limit.to_string(),
        "--json",
        "number,title,repository,url,isDraft,updatedAt",
    ]))
    .ok_or_else(|| "gh search prs failed".to_owned())?;
    parse_prs(&text)
}

pub fn parse_prs(text: &str) -> Result<Vec<Pr>, String> {
    let rows: Vec<Value> = serde_json::from_str(text).map_err(|e| e.to_string())?;
    let s = |v: &Value, k: &str| {
        v.get(k)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    Ok(rows
        .iter()
        .map(|r| Pr {
            repo: r
                .get("repository")
                .and_then(|x| x.get("nameWithOwner"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            number: r.get("number").and_then(Value::as_u64).unwrap_or(0),
            title: s(r, "title"),
            url: s(r, "url"),
            draft: r.get("isDraft").and_then(Value::as_bool).unwrap_or(false),
            updated: s(r, "updatedAt"),
        })
        .collect())
}

/// The text of the last assistant message in a Claude transcript.
pub fn last_said(transcript: &Path) -> Option<String> {
    let mut file = fs::File::open(transcript).ok()?;
    let len = file.metadata().ok()?.len();
    let start = len.saturating_sub(TRANSCRIPT_TAIL_BYTES);
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut buf = Vec::new();
    file.take(TRANSCRIPT_TAIL_BYTES)
        .read_to_end(&mut buf)
        .ok()?;
    last_said_in(&String::from_utf8_lossy(&buf))
}

pub fn last_said_in(jsonl: &str) -> Option<String> {
    jsonl.lines().rev().find_map(|line| {
        let v: Value = serde_json::from_str(line).ok()?;
        if v.get("type")?.as_str()? != "assistant" {
            return None;
        }
        let content = v.get("message")?.get("content")?;
        let text = match content {
            Value::String(s) => s.clone(),
            Value::Array(parts) => parts
                .iter()
                .filter(|p| p.get("type").and_then(Value::as_str) == Some("text"))
                .filter_map(|p| p.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("\n"),
            _ => return None,
        };
        (!text.trim().is_empty()).then(|| clip_block(&text, LAST_SAID_CHARS))
    })
}

/// How much of a transcript's end a session tab shows.
const CONVERSATION_TAIL_BYTES: u64 = 1024 * 1024;

/// One message of a conversation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Turn {
    pub user: bool,
    pub text: String,
    pub at: String,
}

/// The recent conversation in a Claude transcript: what the user typed and
/// what the agent said, oldest first; tool calls and results left out.
pub fn conversation(transcript: &Path, max_turns: usize) -> Vec<Turn> {
    let Ok(mut file) = fs::File::open(transcript) else {
        return Vec::new();
    };
    let Ok(len) = file.metadata().map(|m| m.len()) else {
        return Vec::new();
    };
    if file
        .seek(SeekFrom::Start(len.saturating_sub(CONVERSATION_TAIL_BYTES)))
        .is_err()
    {
        return Vec::new();
    }
    let mut buf = Vec::new();
    if file
        .take(CONVERSATION_TAIL_BYTES)
        .read_to_end(&mut buf)
        .is_err()
    {
        return Vec::new();
    }
    let mut turns = conversation_in(&String::from_utf8_lossy(&buf));
    let skip = turns.len().saturating_sub(max_turns);
    turns.drain(..skip);
    turns
}

pub fn conversation_in(jsonl: &str) -> Vec<Turn> {
    jsonl
        .lines()
        .filter_map(|line| {
            let v: Value = serde_json::from_str(line).ok()?;
            let user = match v.get("type")?.as_str()? {
                "user" => true,
                "assistant" => false,
                _ => return None,
            };
            if v.get("isMeta").and_then(Value::as_bool) == Some(true) {
                return None;
            }
            let text = match v.get("message")?.get("content")? {
                Value::String(s) => s.clone(),
                Value::Array(parts) => parts
                    .iter()
                    .filter(|p| p.get("type").and_then(Value::as_str) == Some("text"))
                    .filter_map(|p| p.get("text").and_then(Value::as_str))
                    .collect::<Vec<_>>()
                    .join("\n\n"),
                _ => return None,
            };
            let text = text.trim();
            // Harness wrappers (command output, reminders) are not conversation.
            if text.is_empty() || text.starts_with('<') {
                return None;
            }
            let at = v
                .get("timestamp")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            Some(Turn {
                user,
                text: text.to_owned(),
                at,
            })
        })
        .collect()
}

/// The task in progress and how many are pending, from
/// `~/.claude/tasks/<session>/*.json`.
pub fn tasks(session_id: &str) -> (Option<String>, u32) {
    let Some(dir) = crate::home().map(|h| h.join(".claude/tasks").join(session_id)) else {
        return (None, 0);
    };
    if session_id.contains(['/', '.']) {
        return (None, 0);
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return (None, 0);
    };
    let mut active = None;
    let mut pending = 0u32;
    for entry in entries.filter_map(Result::ok).take(MAX_TASK_FILES) {
        let Ok(body) = fs::read(entry.path()) else {
            continue;
        };
        let Ok(task) = serde_json::from_slice::<Value>(&body) else {
            continue;
        };
        let field = |k: &str| task.get(k).and_then(Value::as_str);
        match field("status") {
            Some("in_progress") if active.is_none() => {
                active = field("activeForm").or(field("subject")).map(str::to_owned);
            }
            Some("pending") => pending = pending.saturating_add(1),
            _ => {}
        }
    }
    (active, pending)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ps_lines_parse() {
        let procs = parse_ps(
            " 8286  6569 01:02:03 ttys000  claude\n 53002 61892 2-00:00:05 ??  /Users/x/.codex/bin/codex\nbad line\n",
        );
        assert_eq!(procs.len(), 2);
        assert_eq!(procs[0].tty.as_deref(), Some("ttys000"));
        assert_eq!(
            procs[1],
            Proc {
                pid: 53002,
                ppid: 61892,
                tty: None,
                elapsed: 172_805,
                name: "codex".into()
            }
        );
        assert_eq!(procs[0].elapsed, 3723);
        assert_eq!(
            agent_processes(&procs).len(),
            1,
            "daemons without a tty are not sessions"
        );
    }

    #[test]
    fn ancestor_walks_through_the_hook_shell() {
        let procs = parse_ps(
            "10 1 00:09 ttys001 claude\n20 10 00:01 ttys001 sh\n30 20 00:00 ttys001 artalde\n",
        );
        assert_eq!(agent_ancestor(&procs, 20).map(|p| p.pid), Some(10));
        assert_eq!(agent_ancestor(&procs, 999), None);
    }

    #[test]
    fn status_counts() {
        let wt = parse_status(
            Path::new("/r"),
            "# branch.oid abc\n# branch.head feat-x\n# branch.ab +2 -1\n1 .M N... a\n? new.rs\n",
        );
        assert_eq!(
            (wt.branch.as_deref(), wt.ahead, wt.behind, wt.dirty),
            (Some("feat-x"), 2, 1, 2)
        );
    }

    #[test]
    fn prs_parse() {
        let prs = parse_prs(
            r#"[{"number":7,"title":"t","url":"u","isDraft":true,"updatedAt":"2026","repository":{"nameWithOwner":"o/r"}}]"#,
        )
        .unwrap();
        assert_eq!(prs[0].repo, "o/r");
        assert!(prs[0].draft);
    }

    #[test]
    fn conversation_keeps_words_not_tools() {
        let jsonl = [
            r#"{"type":"user","message":{"content":"fix **the** bug"},"timestamp":"t1"}"#,
            r#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Bash"}]}}"#,
            r#"{"type":"user","message":{"content":[{"type":"tool_result","content":"ok"}]}}"#,
            r#"{"type":"user","message":{"content":"<command-name>/clear</command-name>"}}"#,
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Done."}]}}"#,
        ]
        .join("\n");
        let turns = conversation_in(&jsonl);
        assert_eq!(turns.len(), 2);
        assert!(turns[0].user && turns[0].text == "fix **the** bug" && turns[0].at == "t1");
        assert!(!turns[1].user);
    }

    #[test]
    fn last_said_skips_tool_only_turns() {
        let jsonl = [
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"All tests pass."}]}}"#,
            r#"{"type":"user","message":{"content":"ok"}}"#,
            r#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Bash"}]}}"#,
        ]
        .join("\n");
        assert_eq!(last_said_in(&jsonl).as_deref(), Some("All tests pass."));
    }
}
