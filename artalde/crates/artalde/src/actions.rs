//! Side effects outside the window: bringing a terminal forward, typing
//! into it, opening a tab for a new agent, URLs, notifications.
//!
//! They run on one worker thread so a slow `osascript` never stalls a
//! frame. Arguments reach AppleScript as `argv`, never spliced into the
//! script, so no session text can become script.

use std::process::{Command, Stdio};
use std::sync::mpsc::{Receiver, SyncSender, TrySendError, sync_channel};
use std::thread::JoinHandle;

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
    Notify {
        title: String,
        body: String,
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

const NOTIFY: &str = r#"
on run argv
  display notification (item 2 of argv) with title (item 1 of argv) sound name "Glass"
  return "ok"
end run
"#;

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
        Job::Notify { title, body } => osascript(NOTIFY, &[title, body]).map(|_| None),
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
        Job::Notify { .. } => "notify",
    }
}
