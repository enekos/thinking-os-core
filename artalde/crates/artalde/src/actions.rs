//! Side effects outside the window: bringing a terminal forward, typing
//! into it, opening a tab for a new agent, ending one, URLs, notifications.
//!
//! They run on one worker thread so a slow `osascript` never stalls a
//! frame. Arguments reach AppleScript as `argv`, never spliced into the
//! script, so no session text can become script.

use std::process::{Command, Stdio};
use std::sync::mpsc::{Receiver, SyncSender, TrySendError, sync_channel};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

const QUEUE: usize = 32;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Job {
    /// Bring the terminal tab on `tty` (e.g. `ttys004`) to the front.
    Focus {
        tty: String,
    },
    /// Type `text` into the terminal on `tty`, then Enter if `enter`.
    Type {
        tty: String,
        text: String,
        enter: bool,
    },
    /// Open a new terminal tab running `line`.
    Spawn {
        line: String,
    },
    Open {
        target: String,
    },
    /// Run a program without a shell; its failure is logged.
    Run {
        program: String,
        args: Vec<String>,
    },
    /// `open -a <app> <target>`.
    OpenWith {
        app: String,
        target: String,
    },
    /// End an agent: TERM, then KILL if it lingers, then close its iTerm2
    /// tab when `close_tab`. The pid is checked to still be `agent` first.
    Close {
        pid: Option<u32>,
        agent: String,
        tty: Option<String>,
        close_tab: bool,
        name: String,
    },
    /// Colour the terminal tab on `tty` (iTerm2), or reset it with `None`.
    TabColor {
        tty: String,
        rgb: Option<[u8; 3]>,
    },
}

pub struct Actions {
    tx: Option<SyncSender<Job>>,
    pub results: Receiver<String>,
    worker: Option<JoinHandle<()>>,
}

impl Actions {
    pub fn start(ctx: &eframe::egui::Context) -> std::io::Result<Actions> {
        let (tx, rx) = sync_channel::<Job>(QUEUE);
        let (res_tx, results) = sync_channel::<String>(QUEUE);
        let ctx = ctx.clone();
        let worker = std::thread::Builder::new()
            .name("artalde-actions".into())
            .spawn(move || {
                for job in rx {
                    let out = run(&job);
                    if let Some(line) = out {
                        // The UI drains results every frame; a full queue only drops log lines.
                        let _dropped = res_tx.try_send(line);
                        ctx.request_repaint();
                    }
                }
            })?;
        Ok(Actions {
            tx: Some(tx),
            results,
            worker: Some(worker),
        })
    }

    /// Queue `job`; `Err` names why it was not queued.
    pub fn submit(&self, job: Job) -> Result<(), String> {
        let Some(tx) = &self.tx else {
            return Err("actions stopped".into());
        };
        tx.try_send(job).map_err(|e| match e {
            TrySendError::Full(_) => "busy — try again".to_owned(),
            TrySendError::Disconnected(_) => "actions stopped".to_owned(),
        })
    }
}

impl Drop for Actions {
    fn drop(&mut self) {
        // Closing the queue ends the worker's loop.
        self.tx = None;
        if let Some(w) = self.worker.take()
            && w.join().is_err()
        {
            eprintln!("artalde: the actions worker panicked");
        }
    }
}

const FOCUS: &str = r#"
on run argv
  set target to item 1 of argv
  if application "iTerm2" is running then
    tell application "iTerm2"
      repeat with w in windows
        repeat with t in tabs of w
          repeat with s in sessions of t
            if tty of s is target then
              select w
              tell t to select
              tell s to select
              activate
              return "ok"
            end if
          end repeat
        end repeat
      end repeat
    end tell
  end if
  if application "Terminal" is running then
    tell application "Terminal"
      repeat with w in windows
        repeat with t in tabs of w
          if tty of t is target then
            set selected of t to true
            set index of w to 1
            activate
            return "ok"
          end if
        end repeat
      end repeat
    end tell
  end if
  return "no terminal tab on " & target
end run
"#;

const TYPE: &str = r#"
on run argv
  set target to item 1 of argv
  set payload to item 2 of argv
  set enter to (item 3 of argv) is "1"
  if application "iTerm2" is running then
    tell application "iTerm2"
      repeat with w in windows
        repeat with t in tabs of w
          repeat with s in sessions of t
            if tty of s is target then
              if payload is "<esc>" then
                tell s to write text (ASCII character 27) newline NO
              else
                tell s to write text payload newline enter
              end if
              return "ok"
            end if
          end repeat
        end repeat
      end repeat
    end tell
  end if
  return "typing needs iTerm2; no iTerm2 session on " & target
end run
"#;

const SPAWN: &str = r#"
on run argv
  set line to item 1 of argv
  if application "iTerm2" is running then
    tell application "iTerm2"
      if (count of windows) = 0 then
        create window with default profile
      else
        tell current window to create tab with default profile
      end if
      tell current session of current window to write text line
      activate
    end tell
  else
    tell application "Terminal"
      do script line
      activate
    end tell
  end if
  return "ok"
end run
"#;

const CLOSE_TAB: &str = r#"
on run argv
  set target to item 1 of argv
  if application "iTerm2" is running then
    tell application "iTerm2"
      repeat with w in windows
        repeat with t in tabs of w
          repeat with s in sessions of t
            if tty of s is target then
              close s
              return "ok"
            end if
          end repeat
        end repeat
      end repeat
    end tell
  end if
  return "no iTerm2 tab on " & target
end run
"#;

/// How long an agent gets to exit after TERM before KILL.
const TERM_GRACE: Duration = Duration::from_secs(4);
const POLL: Duration = Duration::from_millis(200);

fn alive(pid: u32) -> bool {
    Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

fn signal(pid: u32, sig: &str) -> Result<(), String> {
    let ok = Command::new("kill")
        .args([sig, &pid.to_string()])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|e| e.to_string())?
        .success();
    if ok || !alive(pid) {
        Ok(())
    } else {
        Err(format!("kill {sig} {pid} failed"))
    }
}

/// End `pid` if it is still `agent`; `Ok(false)` when it was already gone.
fn end_process(pid: u32, agent: &str) -> Result<bool, String> {
    let comm = Command::new("ps")
        .args(["-o", "comm=", "-p", &pid.to_string()])
        .stdin(Stdio::null())
        .output()
        .map_err(|e| e.to_string())?;
    let comm = String::from_utf8_lossy(&comm.stdout);
    if comm.trim().is_empty() {
        return Ok(false);
    }
    if !artalde_core::lifecycle::pid_is_agent(&comm, agent) {
        return Err(format!("pid {pid} is now {:?}, not {agent}", comm.trim()));
    }
    signal(pid, "-TERM")?;
    let deadline = Instant::now() + TERM_GRACE;
    while Instant::now() < deadline {
        if !alive(pid) {
            return Ok(true);
        }
        std::thread::sleep(POLL);
    }
    signal(pid, "-KILL")?;
    Ok(true)
}

fn close(
    pid: Option<u32>,
    agent: &str,
    tty: Option<&str>,
    close_tab: bool,
    name: &str,
) -> Result<Option<String>, String> {
    if let Some(pid) = pid {
        end_process(pid, agent)?;
    }
    if close_tab && let Some(tty) = tty {
        // The tab may already be gone with its process; that is fine.
        let _gone_or_closed = osascript(CLOSE_TAB, &[&dev(tty)]);
    }
    Ok(Some(format!("closed {name}")))
}

fn osascript(script: &str, args: &[&str]) -> Result<String, String> {
    let out = Command::new("osascript")
        .arg("-e")
        .arg(script)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("osascript: {e}"))?;
    let stdout = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_owned());
    }
    if stdout == "ok" {
        Ok(stdout)
    } else {
        Err(stdout)
    }
}

/// iTerm2's tab-colour escape, written straight to the session's terminal.
fn tab_color(tty: &str, rgb: Option<[u8; 3]>) -> Result<(), String> {
    use std::io::Write;
    let name = tty.trim_start_matches("/dev/");
    let valid = name.starts_with("tty")
        && name
            .get(3..)
            .is_some_and(|r| r.chars().all(|c| c.is_ascii_alphanumeric()));
    if !valid {
        return Err(format!("{tty:?} is not a terminal"));
    }
    let seq = match rgb {
        Some([r, g, b]) => format!(
            "\x1b]6;1;bg;red;brightness;{r}\x07\x1b]6;1;bg;green;brightness;{g}\x07\x1b]6;1;bg;blue;brightness;{b}\x07"
        ),
        None => "\x1b]6;1;bg;*;default\x07".to_owned(),
    };
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .open(dev(name))
        .map_err(|e| e.to_string())?;
    f.write_all(seq.as_bytes()).map_err(|e| e.to_string())
}

fn dev(tty: &str) -> String {
    if tty.starts_with("/dev/") {
        tty.to_owned()
    } else {
        format!("/dev/{tty}")
    }
}

/// Run one job; returns a log line for failures and notable successes.
fn run(job: &Job) -> Option<String> {
    let result = match job {
        Job::Focus { tty } => osascript(FOCUS, &[&dev(tty)]).map(|_| None),
        Job::Type { tty, text, enter } => {
            osascript(TYPE, &[&dev(tty), text, if *enter { "1" } else { "0" }])
                .map(|_| Some(format!("typed into {tty}")))
        }
        Job::Spawn { line } => osascript(SPAWN, &[line]).map(|_| Some(format!("started: {line}"))),
        Job::OpenWith { app, target } => Command::new("open")
            .args(["-a", app, target])
            .stdin(Stdio::null())
            .status()
            .map_err(|e| e.to_string())
            .and_then(|s| {
                if s.success() {
                    Ok(None)
                } else {
                    Err(format!("open -a {app} failed"))
                }
            }),
        Job::Run { program, args } => Command::new(program)
            .args(args)
            .stdin(Stdio::null())
            .output()
            .map_err(|e| e.to_string())
            .and_then(|o| {
                let text = String::from_utf8_lossy(&o.stdout).trim().to_owned();
                if o.status.success() {
                    Ok(Some(text))
                } else {
                    Err(String::from_utf8_lossy(&o.stderr).trim().to_owned())
                }
            }),
        Job::TabColor { tty, rgb } => tab_color(tty, *rgb).map(|()| None),
        Job::Close {
            pid,
            agent,
            tty,
            close_tab,
            name,
        } => close(*pid, agent, tty.as_deref(), *close_tab, name),
        Job::Open { target } => Command::new("open")
            .arg(target)
            .stdin(Stdio::null())
            .status()
            .map_err(|e| e.to_string())
            .and_then(|s| {
                if s.success() {
                    Ok(None)
                } else {
                    Err(format!("open {target} failed"))
                }
            }),
    };
    match result {
        Ok(line) => line,
        Err(e) => Some(format!("{}: {e}", label(job))),
    }
}

fn label(job: &Job) -> &'static str {
    match job {
        Job::Focus { .. } => "focus",
        Job::Type { .. } => "send",
        Job::Spawn { .. } => "spawn",
        Job::Open { .. } => "open",
        Job::TabColor { .. } => "tab colour",
        Job::OpenWith { .. } => "editor",
        Job::Run { .. } => "run",
        Job::Close { .. } => "close",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(target_os = "macos")]
    fn close_ends_the_agent_and_refuses_a_stranger() {
        let mut agent = Command::new("/bin/sleep").arg("60").spawn().unwrap();
        let pid = agent.id();
        // Reap it as soon as it exits, so `kill -0` stops seeing a zombie.
        let reaper = std::thread::spawn(move || agent.wait());
        std::thread::sleep(Duration::from_millis(200));
        assert_eq!(
            end_process(pid, "claude").map_err(|e| e.contains("not claude")),
            Err(true)
        );
        assert!(alive(pid), "a mismatched name is never signalled");
        assert_eq!(end_process(pid, "sleep"), Ok(true));
        let status = reaper.join().unwrap().unwrap();
        assert!(!status.success(), "ended by a signal");
        assert_eq!(end_process(pid, "sleep"), Ok(false), "already gone");
    }
}
