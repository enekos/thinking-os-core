//! `gh` off the UI thread: one worker for the inbox (slow, periodic), one
//! for PR detail, diffs, logs and writes (fast, on demand), so a refreshing
//! inbox never delays opening a PR. Both are owned and joined on drop.

use std::sync::mpsc::{Receiver, SyncSender, TrySendError, sync_channel};
use std::thread::JoinHandle;

use artalde_core::github::{self, Action, Bucket, DiffFile, PrDetail, PrRef, PrRow};
use eframe::egui;

const QUEUE: usize = 64;

pub enum Req {
    Inbox {
        since: String,
        review_owners: Option<Vec<String>>,
    },
    Detail(PrRef),
    Diff(PrRef),
    Context {
        pr: PrRef,
        file: DiffFile,
        revision: String,
    },
    Repo(String),
    RepoActivity(String),
    Log {
        pr: PrRef,
        job: u64,
    },
    Act {
        pr: PrRef,
        action: Action,
        detail: Option<Box<PrDetail>>,
    },
}

pub enum Res {
    /// One inbox section, as soon as its search returns; `last` ends a refresh.
    InboxBucket {
        login: String,
        bucket: Bucket,
        rows: Result<Vec<PrRow>, String>,
        last: bool,
    },
    Detail(PrRef, Result<Box<PrDetail>, String>),
    Diff(PrRef, Result<Vec<DiffFile>, String>),
    Context {
        pr: PrRef,
        path: String,
        revision: String,
        lines: Result<Vec<github::DiffLine>, String>,
    },
    Repo(
        String,
        Result<Box<artalde_core::github::RepoDetail>, String>,
    ),
    RepoActivity(String, Result<artalde_core::github::RepoActivity, String>),
    Log {
        job: u64,
        log: Result<String, String>,
    },
    Acted {
        pr: PrRef,
        what: String,
        marked_ready: bool,
        result: Result<(), String>,
    },
}

pub struct Gh {
    inbox_tx: Option<SyncSender<Req>>,
    tx: Option<SyncSender<Req>>,
    pub rx: Receiver<Res>,
    workers: Vec<JoinHandle<()>>,
}

impl Gh {
    pub fn start(ctx: &egui::Context) -> std::io::Result<Gh> {
        let (res_tx, rx) = sync_channel::<Res>(QUEUE);
        let mut workers = Vec::new();
        let mut spawn = |name: &str| -> std::io::Result<SyncSender<Req>> {
            let (tx, req_rx) = sync_channel::<Req>(QUEUE);
            let res_tx = res_tx.clone();
            let ctx = ctx.clone();
            workers.push(
                std::thread::Builder::new()
                    .name(name.into())
                    .spawn(move || {
                        for req in req_rx {
                            let mut emit = |res: Res| {
                                let ok = res_tx.send(res).is_ok();
                                ctx.request_repaint();
                                ok
                            };
                            if !handle(req, &mut emit) {
                                return;
                            }
                        }
                    })?,
            );
            Ok(tx)
        };
        let inbox_tx = spawn("artalde-gh-inbox")?;
        let tx = spawn("artalde-gh")?;
        Ok(Gh {
            inbox_tx: Some(inbox_tx),
            tx: Some(tx),
            rx,
            workers,
        })
    }

    pub fn send(&self, req: Req) -> Result<(), String> {
        let tx = if matches!(req, Req::Inbox { .. }) {
            &self.inbox_tx
        } else {
            &self.tx
        };
        let Some(tx) = tx else {
            return Err("gh stopped".into());
        };
        tx.try_send(req).map_err(|e| match e {
            TrySendError::Full(_) => "gh is busy — try again".to_owned(),
            TrySendError::Disconnected(_) => "gh stopped".to_owned(),
        })
    }
}

impl Drop for Gh {
    fn drop(&mut self) {
        self.inbox_tx = None;
        self.tx = None;
        // Unblock a worker waiting to hand back a result.
        while self.rx.try_recv().is_ok() {}
        for w in self.workers.drain(..) {
            if w.join().is_err() {
                eprintln!("artalde: a gh worker panicked");
            }
        }
    }
}

/// Run one request, handing each result to `emit`; false once the UI is gone.
fn handle(req: Req, emit: &mut dyn FnMut(Res) -> bool) -> bool {
    let res = match req {
        Req::Inbox {
            since,
            review_owners,
        } => {
            let buckets: &[Bucket] = if review_owners.is_some() {
                &Bucket::REVIEW_QUEUE
            } else {
                &Bucket::INBOX
            };
            for (i, &bucket) in buckets.iter().enumerate() {
                let fetched = match (&review_owners, bucket) {
                    (Some(owners), Bucket::Ready) => github::fetch_ready(owners),
                    _ => github::fetch_bucket(bucket, &since),
                };
                let (login, rows) = match fetched {
                    Ok((login, rows)) => (login, Ok(rows)),
                    Err(e) => (String::new(), Err(e)),
                };
                let last = i + 1 == buckets.len();
                if !emit(Res::InboxBucket {
                    login,
                    bucket,
                    rows,
                    last,
                }) {
                    return false;
                }
            }
            return true;
        }
        Req::Detail(pr) => {
            let r = github::fetch_detail(&pr).map(Box::new);
            Res::Detail(pr, r)
        }
        Req::Diff(pr) => {
            let r = github::fetch_diff(&pr);
            Res::Diff(pr, r)
        }
        Req::Context { pr, file, revision } => {
            let lines = artalde_core::diff_context::load(&pr, &file, &revision);
            Res::Context {
                pr,
                path: file.path,
                revision,
                lines,
            }
        }
        Req::Repo(repo) => {
            let r = github::fetch_repo(&repo).map(Box::new);
            Res::Repo(repo, r)
        }
        Req::RepoActivity(repo) => {
            let r = github::fetch_repo_activity(&repo);
            Res::RepoActivity(repo, r)
        }
        Req::Log { pr, job } => Res::Log {
            job,
            log: github::fetch_job_log(&pr.repo, job),
        },
        Req::Act { pr, action, detail } => {
            let what = action.describe();
            let mut marked_ready = false;
            let result = action.invocations(&pr, detail.as_deref()).and_then(|invs| {
                for inv in invs {
                    github::exec(&inv).map_err(|e| if marked_ready {
                        format!("PR is ready, but adding the label failed: {e}. Retry from the PR’s More menu.")
                    } else { e })?;
                    marked_ready = matches!(action, Action::Ready | Action::ReadyAndLabel);
                }
                Ok(())
            });
            Res::Acted {
                pr,
                what,
                marked_ready,
                result,
            }
        }
    };
    emit(res)
}
