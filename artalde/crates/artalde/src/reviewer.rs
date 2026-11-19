//! Runs a review agent on a PR in the background: fetches the raw diff,
//! writes `pr.md` and `pr.diff` into the PR's review folder, then runs the
//! configured command (through a login shell, so the agent is on PATH) with
//! the prompt on stdin. The agent is told to stay read-only; its reply is
//! parsed by the caller into proposed drafts.

use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread::JoinHandle;
use std::time::Instant;

use artalde_core::github::{self, Invocation, PrDetail, PrRef};
use artalde_core::review;
use artalde_core::text::shell_quote;
use eframe::egui;

/// What to review and with which agent.
pub struct Job {
    pub pr: PrRef,
    pub agent: String,
    pub command: String,
    pub detail: PrDetail,
    pub dir: PathBuf,
    pub checkout: Option<PathBuf>,
}

pub struct Done {
    pub pr: PrRef,
    pub agent: String,
    pub result: Result<String, String>,
}

/// One run in flight.
pub struct Run {
    pub agent: String,
    pub started: Instant,
    pid: Option<u32>,
    pid_rx: Receiver<u32>,
}

impl Run {
    pub fn pid(&mut self) -> Option<u32> {
        if self.pid.is_none() {
            self.pid = self.pid_rx.try_recv().ok();
        }
        self.pid
    }
}

pub struct Reviewer {
    tx: Sender<Done>,
    pub rx: Receiver<Done>,
    pub runs: HashMap<PrRef, Run>,
    threads: Vec<JoinHandle<()>>,
}

impl Reviewer {
    pub fn new() -> Reviewer {
        let (tx, rx) = channel();
        Reviewer {
            tx,
            rx,
            runs: HashMap::new(),
            threads: Vec::new(),
        }
    }

    /// Start `job.command` (a shell line with `{dir}` and `{checkout}`).
    pub fn start(&mut self, ctx: &egui::Context, job: Job) {
        let Job {
            pr,
            agent,
            command,
            detail,
            dir,
            checkout,
        } = job;
        if self.runs.contains_key(&pr) {
            return;
        }
        let (pid_tx, pid_rx) = channel();
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        let run_pr = pr.clone();
        let run_agent = agent.clone();
        let spawned = std::thread::Builder::new()
            .name("artalde-review".into())
            .spawn(move || {
                let result = run(
                    &run_pr,
                    &command,
                    &detail,
                    &dir,
                    checkout.as_deref(),
                    &pid_tx,
                );
                // The window may be gone; then nobody needs the result.
                let _closed = tx.send(Done {
                    pr: run_pr,
                    agent: run_agent,
                    result,
                });
                ctx.request_repaint();
            });
        match spawned {
            Ok(h) => {
                self.threads.push(h);
                self.runs.insert(
                    pr,
                    Run {
                        agent,
                        started: Instant::now(),
                        pid: None,
                        pid_rx,
                    },
                );
            }
            Err(e) => {
                let _closed = self.tx.send(Done {
                    pr,
                    agent,
                    result: Err(format!("could not start: {e}")),
                });
            }
        }
    }

    /// Stop a run: the agent's process group gets SIGTERM.
    pub fn cancel(&mut self, pr: &PrRef) {
        if let Some(run) = self.runs.get_mut(pr)
            && let Some(pid) = run.pid()
        {
            let killed = Command::new("kill")
                .arg("-TERM")
                .arg(format!("-{pid}"))
                .status();
            if killed.map_or(true, |s| !s.success()) {
                let _best_effort = Command::new("kill")
                    .arg("-TERM")
                    .arg(pid.to_string())
                    .status();
            }
        }
    }

    /// Forget finished threads.
    pub fn reap(&mut self) {
        let (done, running): (Vec<_>, Vec<_>) =
            self.threads.drain(..).partition(JoinHandle::is_finished);
        self.threads = running;
        for h in done {
            if h.join().is_err() {
                eprintln!("artalde: a review thread panicked");
            }
        }
    }
}

impl Drop for Reviewer {
    fn drop(&mut self) {
        let prs: Vec<PrRef> = self.runs.keys().cloned().collect();
        for pr in prs {
            self.cancel(&pr);
        }
        for h in self.threads.drain(..) {
            if h.join().is_err() {
                eprintln!("artalde: a review thread panicked");
            }
        }
    }
}

pub(crate) fn run(
    pr: &PrRef,
    command: &str,
    detail: &PrDetail,
    dir: &std::path::Path,
    checkout: Option<&std::path::Path>,
    pid_tx: &Sender<u32>,
) -> Result<String, String> {
    let diff = github::exec(&Invocation {
        args: vec![
            "pr".into(),
            "diff".into(),
            pr.number.to_string(),
            "--repo".into(),
            pr.repo.clone(),
        ],
        stdin: None,
    })?;
    review::write_context(dir, detail, &diff).map_err(|e| format!("writing context: {e}"))?;
    let prompt = review::prompt(pr, dir, checkout);
    let line = command
        .replace("{dir}", &shell_quote(&dir.display().to_string()))
        .replace(
            "{checkout}",
            &shell_quote(&checkout.unwrap_or(dir).display().to_string()),
        )
        .replace("{pr}", &shell_quote(&pr.to_string()));
    let log = std::fs::File::create(dir.join("agent.log")).map_err(|e| e.to_string())?;
    let mut child = Command::new("/bin/zsh")
        .arg("-lc")
        // `exec` so the agent itself gets the pid we may signal.
        .arg(format!("exec {line}"))
        .current_dir(dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::from(log))
        .process_group(0)
        .spawn()
        .map_err(|e| format!("starting the agent: {e}"))?;
    let _closed = pid_tx.send(child.id());
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(prompt.as_bytes())
            .map_err(|e| format!("sending the prompt: {e}"))?;
    }
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    std::fs::write(dir.join("agent-reply.md"), &text).map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(text)
    } else {
        let tail = std::fs::read_to_string(dir.join("agent.log")).unwrap_or_default();
        let tail: Vec<&str> = tail.lines().rev().take(6).collect();
        Err(format!(
            "agent exited with {}: {}",
            out.status,
            tail.into_iter().rev().collect::<Vec<_>>().join(" / ")
        ))
    }
}

/// `CommandExt::process_group` lives in an extension trait.
use std::os::unix::process::CommandExt as _;
