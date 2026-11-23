//! The host: owns the terminals and serves them on a unix socket.
//!
//! # Threads
//!
//! - one that accepts connections;
//! - per client, a reader (which also runs its requests) and a writer;
//! - per terminal, a reader that always drains the PTY, a writer for its
//!   input, and one that waits for the child;
//! - a keeper for everything periodic, which also runs the shutdown.
//!
//! # Locks
//!
//! Registry, then one terminal, then the subscriber list, then a client's
//! frame queue. The registry lock is never held together with a terminal's:
//! the terminal is looked up, the registry released, then the terminal
//! locked. Nothing that can block is done under any of them: frames go to a
//! queue, which the client's writer thread empties.
//!
//! Events about a terminal are published under its lock, so subscribers see
//! them in the order the state changed.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::fs::{self, File};
use std::io::{self, PipeReader, PipeWriter};
use std::net::Shutdown;
use std::os::unix::fs::PermissionsExt as _;
use std::os::unix::net::{UnixListener, UnixStream};
use std::os::unix::process::ExitStatusExt as _;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use rustix::event::{PollFd, PollFlags, Timespec};
use rustix::fs::FlockOperation;
use rustix::io::Errno;
use rustix::process::{Pid, Signal, WaitId, WaitIdOptions};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::paths;
use crate::procinfo;
use crate::pty::Pty;
use crate::queue::{FrameQueue, Push};
use crate::screen::{Screen, ScreenEvent, StatusReport};
use crate::util::{LockExt as _, log, now};
use crate::wire::{
    Ended, Event, Frame, HostInfo, Kind, PROTOCOL_VERSION, State, Status, StatusState,
    TerminalInfo, decode_resize, read_frame,
};

/// Output queued for one viewer of one terminal before its backlog is
/// replaced by a fresh replay.
const VIEWER_BACKLOG: usize = 4 << 20;
/// Replies and events queued for one client before it is dropped.
const CONTROL_BACKLOG: usize = 32 << 20;
/// Input queued for one terminal.
const INPUT_BACKLOG: usize = 4 << 20;
const READ_CHUNK: usize = 64 << 10;
/// How long output is still read after the child is gone.
const DRAIN_GRACE: Duration = Duration::from_millis(200);
/// Between a pasted text and the Enter that submits it: programs that read
/// the two in one go take the Enter for part of the paste.
const SUBMIT_GAP: Duration = Duration::from_millis(200);
const NUDGE_GAP: Duration = Duration::from_millis(50);
const KILL_STEP: Duration = Duration::from_millis(500);
const KILL_LAST: Duration = Duration::from_secs(2);
const KEEP_FINISHED_SECS: u64 = 24 * 60 * 60;
const KEEP_FINISHED: usize = 200;
/// Finished terminals whose output can still be read and attached to.
const KEEP_SCREENS: usize = 32;
const MAX_RUNNING: usize = 512;
const MAX_CLIENTS: usize = 256;
const TICK: Duration = Duration::from_millis(250);
/// How often and how long apart a held lock is tried again before it is
/// taken for another host's.
const LOCK_TRIES: u32 = 20;
const LOCK_RETRY: Duration = Duration::from_millis(10);
const SAVE_DEBOUNCE: Duration = Duration::from_secs(1);
/// Ids reserved on disk at a time, so most `run`s write nothing.
const ID_BLOCK: u32 = 16;
const REGISTRY_VERSION: u32 = 1;

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum HostError {
    /// Another host holds the lock of this state directory.
    #[error("a host is already running for {}", .0.display())]
    AlreadyRunning(PathBuf),
    #[error("{context}: {source}")]
    Io {
        context: String,
        #[source]
        source: io::Error,
    },
}

fn io_err(context: impl Into<String>) -> impl FnOnce(io::Error) -> HostError {
    let context = context.into();
    move |source| HostError::Io { context, source }
}

/// The terminal host. See the crate documentation.
#[derive(Debug)]
pub struct Host;

impl Host {
    /// Starts a host for `state_dir` on threads of this process and returns
    /// once it accepts connections.
    pub fn serve(state_dir: &Path) -> Result<HostHandle, HostError> {
        paths::prepare_state(state_dir).map_err(io_err("state directory"))?;
        let lock_path = paths::lock_file(state_dir);
        let lock = File::options()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&lock_path)
            .map_err(io_err("open lock file"))?;
        // Not every holder is a host: a client asking whether one runs holds
        // the lock shared for a moment, and so does a child of a host that
        // just stopped, between its fork and its exec. Those pass.
        let mut tries = 0;
        loop {
            match rustix::fs::flock(&lock, FlockOperation::NonBlockingLockExclusive) {
                Ok(()) => break,
                Err(Errno::WOULDBLOCK) if tries < LOCK_TRIES => {
                    tries += 1;
                    std::thread::sleep(LOCK_RETRY);
                }
                Err(Errno::WOULDBLOCK) => {
                    return Err(HostError::AlreadyRunning(state_dir.to_path_buf()));
                }
                Err(Errno::INTR) => {}
                Err(e) => return Err(io_err("lock")(e.into())),
            }
        }

        let socket = paths::socket_for_host(state_dir).map_err(io_err("socket directory"))?;
        // Holding the lock, any socket there is a dead host's.
        fs::remove_file(&socket).ok();
        let listener = UnixListener::bind(&socket).map_err(io_err("bind socket"))?;
        fs::set_permissions(&socket, fs::Permissions::from_mode(0o600))
            .map_err(io_err("socket permissions"))?;
        listener
            .set_nonblocking(true)
            .map_err(io_err("listen on socket"))?;
        let addr = format!("{}\n", socket.display());
        paths::write_atomic(&paths::addr_file(state_dir), addr.as_bytes())
            .map_err(io_err("write host.addr"))?;

        let (terms, high) = load_registry(state_dir);
        let next_id = fs::read_to_string(paths::seq_file(state_dir))
            .ok()
            .and_then(|s| s.trim().parse::<u32>().ok())
            .unwrap_or(1)
            .max(high.saturating_add(1));
        let (accept_stop_r, accept_stop_w) = io::pipe().map_err(io_err("pipe"))?;
        let shared = Arc::new(Shared {
            state_dir: state_dir.to_path_buf(),
            socket,
            started: now(),
            registry: Mutex::new(Registry {
                terms,
                closed: false,
            }),
            subscribers: Mutex::new(Vec::new()),
            clients: Mutex::new(HashMap::new()),
            spawner: Mutex::new(Spawner {
                next: next_id,
                reserved: next_id,
            }),
            threads: Mutex::new(Vec::new()),
            phase: Mutex::new(Phase::Running),
            phase_changed: Condvar::new(),
            accept_stop: Mutex::new(Some(accept_stop_w)),
            lock: Mutex::new(Some(lock)),
            dirty: AtomicBool::new(true),
            frozen: AtomicBool::new(false),
            next_client: AtomicU64::new(1),
        });

        let for_accept = Arc::clone(&shared);
        let accept = shared.spawn("accept", move || {
            accept_loop(&for_accept, &listener, &accept_stop_r);
        });
        let for_keeper = Arc::clone(&shared);
        let keeper = thread::Builder::new()
            .name("artalde-host keeper".into())
            .spawn(move || keeper_loop(&for_keeper));
        match (accept, keeper) {
            (Ok(()), Ok(keeper)) => {
                log!(
                    "serving {} (pid {})",
                    shared.socket.display(),
                    std::process::id()
                );
                Ok(HostHandle {
                    shared,
                    keeper: Some(keeper),
                })
            }
            (accept, keeper) => {
                shared.request_stop();
                match keeper {
                    Ok(keeper) => {
                        keeper.join().ok();
                    }
                    // Nothing runs the shutdown; do the part that matters.
                    Err(_) => teardown(&shared),
                }
                let source = accept
                    .err()
                    .unwrap_or_else(|| io::Error::other("could not start the keeper thread"));
                Err(io_err("start threads")(source))
            }
        }
    }

    /// Runs a host for `state_dir` until it is asked to stop, by a
    /// `shutdown` request or by `SIGTERM` or `SIGINT`. For a process whose
    /// only job this is: it takes over those signals, swallows `SIGHUP` and
    /// `SIGPIPE`, and raises the open-file limit.
    pub fn run(state_dir: &Path) -> Result<(), HostError> {
        use signal_hook::consts::{SIGHUP, SIGINT, SIGPIPE, SIGTERM};

        raise_nofile();
        // Handlers rather than `SIG_IGN`: an ignored signal stays ignored in
        // the programs the host starts, a handled one goes back to default.
        let mut signals = signal_hook::iterator::Signals::new([SIGTERM, SIGINT, SIGHUP, SIGPIPE])
            .map_err(io_err("signal handlers"))?;
        let handle = Host::serve(state_dir)?;
        let closer = signals.handle();
        let shared = Arc::clone(&handle.shared);
        let watcher = thread::Builder::new()
            .name("artalde-host signals".into())
            .spawn(move || {
                for signal in signals.forever() {
                    if signal == SIGTERM || signal == SIGINT {
                        log!("signal {signal}, stopping");
                        shared.request_stop();
                    }
                }
            })
            .map_err(io_err("signal thread"))?;
        handle.wait();
        closer.close();
        watcher.join().ok();
        handle.stop();
        Ok(())
    }
}

/// A host started by [`Host::serve`]. Dropping it stops the host.
#[derive(Debug)]
pub struct HostHandle {
    shared: Arc<Shared>,
    keeper: Option<JoinHandle<()>>,
}

impl HostHandle {
    /// The socket the host listens on.
    pub fn socket(&self) -> &Path {
        &self.shared.socket
    }

    /// Blocks until the host has stopped: after a `shutdown` request, or
    /// [`HostHandle::stop`] from another thread's point of view.
    pub fn wait(&self) {
        let mut phase = self.shared.phase.locked();
        while *phase != Phase::Stopped {
            phase = self
                .shared
                .phase_changed
                .wait(phase)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
        }
    }

    /// Ends every terminal, disconnects every client, saves the registry and
    /// returns when all the host's threads are gone.
    pub fn stop(mut self) {
        self.finish();
    }

    /// Stops like a host that was killed: the registry on disk keeps saying
    /// that the terminals are running. The programs are still ended. For
    /// testing what the next host makes of it.
    pub fn abandon(mut self) {
        self.shared.save();
        self.shared.frozen.store(true, Ordering::Relaxed);
        self.finish();
    }

    fn finish(&mut self) {
        self.shared.request_stop();
        if let Some(keeper) = self.keeper.take() {
            keeper.join().ok();
        }
    }
}

impl Drop for HostHandle {
    fn drop(&mut self) {
        self.finish();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Running,
    Stopping,
    Stopped,
}

#[derive(Debug)]
struct Registry {
    terms: BTreeMap<u32, Arc<Terminal>>,
    /// The host is stopping: no new terminals.
    closed: bool,
}

#[derive(Debug)]
struct Spawner {
    next: u32,
    /// Ids below this are spoken for on disk.
    reserved: u32,
}

#[derive(Debug)]
struct Shared {
    state_dir: PathBuf,
    socket: PathBuf,
    started: u64,
    registry: Mutex<Registry>,
    subscribers: Mutex<Vec<(u64, Arc<Outbox>)>>,
    clients: Mutex<HashMap<u64, Arc<Outbox>>>,
    /// Serialises id allocation, opening a PTY and forking.
    spawner: Mutex<Spawner>,
    threads: Mutex<Vec<JoinHandle<()>>>,
    phase: Mutex<Phase>,
    phase_changed: Condvar,
    /// Dropped to stop the accept thread.
    accept_stop: Mutex<Option<PipeWriter>>,
    lock: Mutex<Option<File>>,
    /// The registry on disk is behind.
    dirty: AtomicBool,
    /// Never write the registry again.
    frozen: AtomicBool,
    next_client: AtomicU64,
}

impl Shared {
    /// Starts a thread the host joins when it stops.
    fn spawn(&self, name: &str, body: impl FnOnce() + Send + 'static) -> io::Result<()> {
        let handle = thread::Builder::new()
            .name(format!("artalde-host {name}"))
            .spawn(body)?;
        self.threads.locked().push(handle);
        Ok(())
    }

    fn request_stop(&self) {
        let mut phase = self.phase.locked();
        if *phase == Phase::Running {
            *phase = Phase::Stopping;
        }
        drop(phase);
        self.phase_changed.notify_all();
    }

    fn publish(&self, event: &Event) {
        let subscribers = self.subscribers.locked();
        if subscribers.is_empty() {
            return;
        }
        let payload = serde_json::to_vec(event).unwrap_or_default();
        for (_, out) in subscribers.iter() {
            out.send(Frame::new(Kind::Event, 0, payload.clone()));
        }
    }

    fn touch(&self) {
        self.dirty.store(true, Ordering::Relaxed);
    }

    fn terminal(&self, id: u32) -> Result<Arc<Terminal>, String> {
        self.registry
            .locked()
            .terms
            .get(&id)
            .map(Arc::clone)
            .ok_or_else(|| format!("no terminal {id}"))
    }

    fn terminals(&self) -> Vec<Arc<Terminal>> {
        self.registry
            .locked()
            .terms
            .values()
            .map(Arc::clone)
            .collect()
    }

    fn infos(&self) -> Vec<TerminalInfo> {
        self.terminals()
            .iter()
            .map(|t| t.st.locked().info.clone())
            .collect()
    }

    fn save(&self) {
        if self.frozen.load(Ordering::Relaxed) {
            return;
        }
        self.dirty.store(false, Ordering::Relaxed);
        let file = RegistryFile {
            version: REGISTRY_VERSION,
            terminals: self.infos(),
        };
        let result = serde_json::to_vec_pretty(&file)
            .map_err(io::Error::other)
            .and_then(|bytes| paths::write_atomic(&paths::registry_file(&self.state_dir), &bytes));
        if let Err(e) = result {
            log!("saving the registry: {e}");
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct RegistryFile {
    version: u32,
    terminals: Vec<TerminalInfo>,
}

/// The terminals of earlier hosts, and the highest id among them. One that
/// was running when its host went away is reported as lost.
fn load_registry(state_dir: &Path) -> (BTreeMap<u32, Arc<Terminal>>, u32) {
    let path = paths::registry_file(state_dir);
    let mut terms = BTreeMap::new();
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return (terms, 0),
        Err(e) => {
            log!("reading {}: {e}", path.display());
            return (terms, 0);
        }
    };
    let file: RegistryFile = match serde_json::from_slice(&bytes) {
        Ok(file) => file,
        Err(e) => {
            log!("{} is not a registry, starting empty: {e}", path.display());
            return (terms, 0);
        }
    };
    let mut high = 0;
    for mut info in file.terminals {
        if info.state.is_running() {
            info.state = State::Exited {
                code: None,
                signal: None,
                at: now(),
                lost: true,
            };
        }
        info.pid = None;
        info.status = None;
        info.foreground = None;
        info.viewers = 0;
        high = high.max(info.id);
        terms.insert(info.id, Arc::new(Terminal::finished(info)));
    }
    (terms, high)
}

/// A client's way out: the queue its writer thread empties.
#[derive(Debug)]
struct Outbox {
    queue: FrameQueue,
    socket: UnixStream,
}

impl Outbox {
    /// Queues a reply or an event. A client that lets these pile up is not
    /// reading at all and is dropped.
    fn send(&self, frame: Frame) {
        if self.queue.push(frame) && self.queue.control_bytes() > CONTROL_BACKLOG {
            self.kick();
        }
    }

    /// Drops the client: its reader and writer threads both wake and end.
    fn kick(&self) {
        self.queue.abort();
        self.socket.shutdown(Shutdown::Both).ok();
    }
}

#[derive(Debug)]
struct Viewer {
    client: u64,
    out: Arc<Outbox>,
    cols: u16,
    rows: u16,
    xpixel: u16,
    ypixel: u16,
    history: bool,
}

#[derive(Debug)]
enum InputCmd {
    Bytes(Vec<u8>),
    /// Text to paste, bracketed when the program asked for that.
    Paste(Vec<u8>),
    Gap(Duration),
    /// Makes a full-screen program repaint: a resize and back.
    Nudge,
}

impl InputCmd {
    fn cost(&self) -> usize {
        match self {
            InputCmd::Bytes(b) | InputCmd::Paste(b) => b.len() + 16,
            InputCmd::Gap(_) | InputCmd::Nudge => 16,
        }
    }
}

#[derive(Debug, Default)]
struct InputState {
    cmds: VecDeque<InputCmd>,
    bytes: usize,
    closed: bool,
}

#[derive(Debug, Default)]
struct InputQueue {
    state: Mutex<InputState>,
    ready: Condvar,
}

impl InputQueue {
    /// Queues all of `cmds` or, when they do not fit, none.
    fn push(&self, cmds: Vec<InputCmd>) -> bool {
        let cost: usize = cmds.iter().map(InputCmd::cost).sum();
        let mut state = self.state.locked();
        if state.closed || state.bytes + cost > INPUT_BACKLOG {
            return false;
        }
        state.bytes += cost;
        state.cmds.extend(cmds);
        drop(state);
        self.ready.notify_all();
        true
    }

    fn pop(&self) -> Option<InputCmd> {
        let mut state = self.state.locked();
        loop {
            if state.closed {
                return None;
            }
            if let Some(cmd) = state.cmds.pop_front() {
                state.bytes = state.bytes.saturating_sub(cmd.cost());
                return Some(cmd);
            }
            state = self
                .ready
                .wait(state)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
        }
    }

    fn close(&self) {
        let mut state = self.state.locked();
        state.closed = true;
        state.cmds.clear();
        drop(state);
        self.ready.notify_all();
    }
}

#[derive(Debug)]
struct Terminal {
    id: u32,
    st: Mutex<TermState>,
    input: InputQueue,
}

#[derive(Debug)]
struct TermState {
    info: TerminalInfo,
    /// Kept after the exit for the last few terminals, so their output can
    /// still be read.
    screen: Option<Screen>,
    pty: Option<Arc<Pty>>,
    child: Option<Child>,
    /// The child has been waited for: its pid means nothing any more and
    /// must not be signalled.
    reaped: bool,
    exit: Option<Ended>,
    /// Dropped when the child is gone, which wakes the reader and the input
    /// writer.
    gone: Option<PipeWriter>,
    viewers: Vec<Viewer>,
    /// The client whose size the PTY has.
    leader: Option<u64>,
    /// Times the status left `working`.
    turn: u32,
    foreground: Option<Pid>,
    /// The program reports its directory itself (`OSC 7`); stop guessing.
    cwd_reported: bool,
}

impl Terminal {
    fn finished(info: TerminalInfo) -> Terminal {
        let input = InputQueue::default();
        input.close();
        Terminal {
            id: info.id,
            st: Mutex::new(TermState {
                info,
                screen: None,
                pty: None,
                child: None,
                reaped: true,
                exit: None,
                gone: None,
                viewers: Vec::new(),
                leader: None,
                turn: 0,
                foreground: None,
                cwd_reported: false,
            }),
            input,
        }
    }
}

impl TermState {
    /// Gives the PTY the size of `client`'s view, when it has one.
    fn follow(&mut self, client: u64, shared: &Shared) {
        self.leader = Some(client);
        let Some(viewer) = self.viewers.iter().find(|v| v.client == client) else {
            return;
        };
        let (cols, rows, xpixel, ypixel) = (viewer.cols, viewer.rows, viewer.xpixel, viewer.ypixel);
        if cols == 0 || rows == 0 || (cols, rows) == (self.info.cols, self.info.rows) {
            return;
        }
        let Some(pty) = &self.pty else { return };
        if let Err(e) = pty.resize(cols, rows, xpixel, ypixel) {
            log!("terminal {}: resize: {e}", self.info.id);
            return;
        }
        self.info.cols = cols;
        self.info.rows = rows;
        if let Some(screen) = &mut self.screen {
            screen.resize(cols, rows);
        }
        shared.publish(&Event::Size {
            terminal: self.info.id,
            cols,
            rows,
        });
        shared.touch();
    }

    fn set_status(&mut self, report: Option<StatusReport>, shared: &Shared) {
        let before = self.info.status.take();
        let state = report.as_ref().and_then(|r| r.state);
        let was = before.as_ref().map(|s| s.state);
        if was == Some(StatusState::Working) && state != Some(StatusState::Working) {
            self.turn += 1;
        }
        self.info.status = report.and_then(|r| {
            Some(Status {
                state: r.state?,
                kind: r.kind,
                app: r.app,
                title: r.title,
                message: r.message,
                progress: r.progress,
                turn: self.turn,
                since: before
                    .as_ref()
                    .filter(|b| Some(b.state) == r.state)
                    .map_or_else(now, |b| b.since),
            })
        });
        if self.info.status != before {
            shared.publish(&Event::Status {
                terminal: self.info.id,
                status: self.info.status.clone(),
            });
            shared.touch();
        }
    }

    fn remove_viewer(&mut self, client: u64) {
        self.viewers.retain(|v| v.client != client);
        self.info.viewers = u32::try_from(self.viewers.len()).unwrap_or(u32::MAX);
        if self.leader == Some(client) {
            self.leader = None;
        }
    }
}

fn accept_loop(shared: &Arc<Shared>, listener: &UnixListener, stop: &PipeReader) {
    loop {
        let mut fds = [
            PollFd::new(listener, PollFlags::IN),
            PollFd::new(stop, PollFlags::IN),
        ];
        match rustix::event::poll(&mut fds, None) {
            Ok(_) => {}
            Err(Errno::INTR) => continue,
            Err(e) => {
                log!("accept: poll: {e}");
                return;
            }
        }
        if !fds[1].revents().is_empty() {
            return;
        }
        let stream = match listener.accept() {
            Ok((stream, _)) => stream,
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => continue,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => {
                log!("accept: {e}");
                // Out of descriptors, most likely; do not spin on it.
                thread::sleep(Duration::from_millis(100));
                continue;
            }
        };
        if let Err(e) = start_client(shared, stream) {
            log!("client: {e}");
        }
    }
}

fn start_client(shared: &Arc<Shared>, stream: UnixStream) -> io::Result<()> {
    // On some platforms an accepted socket inherits the listener's mode.
    stream.set_nonblocking(false)?;
    let id = shared.next_client.fetch_add(1, Ordering::Relaxed);
    let out = Arc::new(Outbox {
        queue: FrameQueue::default(),
        socket: stream.try_clone()?,
    });
    let write_half = stream.try_clone()?;
    {
        let mut clients = shared.clients.locked();
        if clients.len() >= MAX_CLIENTS {
            return Err(io::Error::other("too many clients"));
        }
        clients.insert(id, Arc::clone(&out));
    }
    let for_reader = Arc::clone(shared);
    let started = shared.spawn("client", move || {
        let writer_out = Arc::clone(&out);
        let writer = thread::Builder::new()
            .name("artalde-host client writer".into())
            .spawn(move || {
                writer_out.queue.write_to(&write_half);
                // Whatever ended the writing, the reader must end too.
                writer_out.socket.shutdown(Shutdown::Both).ok();
            });
        match writer {
            Ok(writer) => {
                Conn {
                    id,
                    out: Arc::clone(&out),
                    shared: Arc::clone(&for_reader),
                    attached: HashMap::new(),
                }
                .serve(stream);
                out.kick();
                writer.join().ok();
            }
            Err(e) => log!("client {id}: writer thread: {e}"),
        }
        for_reader.clients.locked().remove(&id);
    });
    if started.is_err() {
        shared.clients.locked().remove(&id);
    }
    started
}

#[derive(Debug, Deserialize)]
struct Request {
    id: u64,
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Debug, Deserialize)]
struct HelloParams {
    #[serde(default)]
    client: String,
    version: u32,
}

fn default_cols() -> u16 {
    80
}

fn default_rows() -> u16 {
    24
}

#[derive(Debug, Deserialize)]
struct RunParams {
    #[serde(default)]
    argv: Vec<String>,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    env: BTreeMap<String, String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default = "default_cols")]
    cols: u16,
    #[serde(default = "default_rows")]
    rows: u16,
    #[serde(default)]
    labels: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
struct TerminalParams {
    terminal: u32,
}

#[derive(Debug, Deserialize)]
struct AttachParams {
    terminal: u32,
    #[serde(default)]
    cols: u16,
    #[serde(default)]
    rows: u16,
    #[serde(default)]
    history: bool,
}

fn default_lines() -> usize {
    100
}

#[derive(Debug, Deserialize)]
struct ReadParams {
    terminal: u32,
    #[serde(default = "default_lines")]
    lines: usize,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Deserialize)]
struct SendParams {
    terminal: u32,
    text: String,
    #[serde(default = "yes")]
    submit: bool,
}

#[derive(Debug, Deserialize)]
struct RemoveParams {
    terminal: u32,
    #[serde(default)]
    force: bool,
}

#[derive(Debug, Deserialize)]
struct RenameParams {
    terminal: u32,
    name: String,
}

#[derive(Debug, Deserialize)]
struct LabelParams {
    terminal: u32,
    key: String,
    #[serde(default)]
    value: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct ShutdownParams {
    #[serde(default)]
    force: bool,
}

fn params<T: serde::de::DeserializeOwned>(value: Value) -> Result<T, String> {
    let value = if value.is_null() { json!({}) } else { value };
    serde_json::from_value(value).map_err(|e| format!("bad params: {e}"))
}

fn result<T: Serialize>(value: &T) -> Result<Value, String> {
    serde_json::to_value(value).map_err(|e| e.to_string())
}

fn reply(out: &Outbox, id: u64, outcome: Result<Value, String>) {
    let body = match outcome {
        Ok(result) => json!({ "id": id, "result": result }),
        Err(error) => json!({ "id": id, "error": error }),
    };
    out.send(Frame::json(Kind::Reply, 0, &body));
}

/// One connection, on its reader thread.
struct Conn {
    id: u64,
    out: Arc<Outbox>,
    shared: Arc<Shared>,
    /// The terminals this client views.
    attached: HashMap<u32, Arc<Terminal>>,
}

impl Conn {
    fn serve(mut self, mut stream: UnixStream) {
        loop {
            let frame = match read_frame(&mut stream) {
                Ok(Some(frame)) => frame,
                Ok(None) => break,
                Err(e) => {
                    log!("client {}: dropped: {e}", self.id);
                    break;
                }
            };
            match frame.kind {
                Kind::Request => {
                    let request: Request = match serde_json::from_slice(&frame.payload) {
                        Ok(request) => request,
                        Err(e) => {
                            log!("client {}: dropped: bad request: {e}", self.id);
                            break;
                        }
                    };
                    let id = request.id;
                    if let Some(outcome) = self.request(request) {
                        reply(&self.out, id, outcome);
                    }
                }
                Kind::Input => self.input(frame.terminal, frame.payload),
                Kind::Resize => {
                    if let Some(size) = decode_resize(&frame.payload) {
                        self.resize(frame.terminal, size);
                    }
                }
                Kind::Detach => self.detach(frame.terminal),
                Kind::Reply | Kind::Event | Kind::Output | Kind::Ended => {
                    log!(
                        "client {}: dropped: sent a host frame ({:?})",
                        self.id,
                        frame.kind
                    );
                    break;
                }
            }
        }
        self.shared
            .subscribers
            .locked()
            .retain(|(client, _)| *client != self.id);
        for term in self.attached.values() {
            term.st.locked().remove_viewer(self.id);
        }
    }

    /// Runs a request. `None` when the reply is sent some other way.
    fn request(&mut self, request: Request) -> Option<Result<Value, String>> {
        let Request { id, method, params } = request;
        Some(match method.as_str() {
            "hello" => self.hello(params),
            "run" => self::params(params)
                .and_then(|p| spawn_terminal(&self.shared, p))
                .and_then(|info| result(&info)),
            "list" => result(&self.shared.infos()),
            "get" => self
                .terminal(params)
                .and_then(|t| result(&t.st.locked().info)),
            "attach" => match self.attach(id, params) {
                Ok(()) => return None,
                Err(e) => Err(e),
            },
            "read" => self.read(params),
            "send" => self.send(params),
            "kill" => match self.terminal(params) {
                Ok(term) => return self.kill(id, term, false),
                Err(e) => Err(e),
            },
            "remove" => match self::params::<RemoveParams>(params) {
                Ok(p) => return self.remove(id, &p),
                Err(e) => Err(e),
            },
            "rename" => self.rename(params),
            "label" => self.label(params),
            "subscribe" => {
                let mut subscribers = self.shared.subscribers.locked();
                if !subscribers.iter().any(|(client, _)| *client == self.id) {
                    subscribers.push((self.id, Arc::clone(&self.out)));
                }
                Ok(Value::Null)
            }
            "shutdown" => self.shutdown(params),
            other => Err(format!("unknown method {other}")),
        })
    }

    fn terminal(&self, params: Value) -> Result<Arc<Terminal>, String> {
        let p: TerminalParams = self::params(params)?;
        self.shared.terminal(p.terminal)
    }

    fn hello(&self, params: Value) -> Result<Value, String> {
        let p: HelloParams = self::params(params)?;
        if p.version != PROTOCOL_VERSION {
            return Err(format!(
                "protocol version mismatch: host speaks {PROTOCOL_VERSION}, {} speaks {}",
                if p.client.is_empty() {
                    "client"
                } else {
                    &p.client
                },
                p.version
            ));
        }
        result(&HostInfo {
            version: PROTOCOL_VERSION,
            pid: std::process::id(),
            started: self.shared.started,
        })
    }

    /// Reply, replay and the start of the live stream are queued under the
    /// terminal's lock, which the PTY reader needs to queue output: nothing
    /// is lost or doubled between replay and live.
    fn attach(&mut self, request: u64, params: Value) -> Result<(), String> {
        let p: AttachParams = self::params(params)?;
        let term = self.shared.terminal(p.terminal)?;
        let mut st = term.st.locked();
        let Some(screen) = &st.screen else {
            return Err(format!(
                "terminal {} has finished and its output is no longer kept",
                term.id
            ));
        };
        let replay = screen.replay(p.history);
        let nudge = screen.alt_screen();
        st.remove_viewer(self.id);
        let running = st.info.state.is_running();
        if running {
            st.viewers.push(Viewer {
                client: self.id,
                out: Arc::clone(&self.out),
                cols: p.cols,
                rows: p.rows,
                xpixel: 0,
                ypixel: 0,
                history: p.history,
            });
            st.info.viewers = u32::try_from(st.viewers.len()).unwrap_or(u32::MAX);
            let before = (st.info.cols, st.info.rows);
            st.follow(self.id, &self.shared);
            // A full-screen program repaints on a size change; when the
            // size did not change, fake one, or the viewer is left with
            // whatever the raw tail happens to paint.
            if nudge && before == (st.info.cols, st.info.rows) {
                term.input.push(vec![InputCmd::Nudge]);
            }
        }
        reply(&self.out, request, result(&st.info));
        self.out.queue.replace_output(term.id, replay);
        if let State::Exited { code, signal, .. } = st.info.state {
            self.out
                .send(Frame::json(Kind::Ended, term.id, &Ended { code, signal }));
        }
        drop(st);
        if running {
            self.attached.insert(term.id, term);
        } else {
            self.attached.remove(&term.id);
        }
        Ok(())
    }

    fn detach(&mut self, terminal: u32) {
        if let Some(term) = self.attached.remove(&terminal) {
            term.st.locked().remove_viewer(self.id);
        }
        // After the viewer is gone nothing adds to this stream.
        self.out.queue.drop_output(terminal);
    }

    fn input(&self, terminal: u32, bytes: Vec<u8>) {
        let Some(term) = self.attached.get(&terminal) else {
            return;
        };
        {
            let mut st = term.st.locked();
            if st.leader != Some(self.id) {
                st.follow(self.id, &self.shared);
            }
        }
        // Typing faster than the program reads, megabytes over: dropped.
        term.input.push(vec![InputCmd::Bytes(bytes)]);
    }

    fn resize(&self, terminal: u32, [cols, rows, cell_w, cell_h]: [u16; 4]) {
        let Some(term) = self.attached.get(&terminal) else {
            return;
        };
        let mut st = term.st.locked();
        if let Some(viewer) = st.viewers.iter_mut().find(|v| v.client == self.id) {
            viewer.cols = cols;
            viewer.rows = rows;
            viewer.xpixel = cols.saturating_mul(cell_w);
            viewer.ypixel = rows.saturating_mul(cell_h);
        }
        st.follow(self.id, &self.shared);
    }

    fn read(&self, params: Value) -> Result<Value, String> {
        let p: ReadParams = self::params(params)?;
        let term = self.shared.terminal(p.terminal)?;
        let st = term.st.locked();
        let screen = st.screen.as_ref().ok_or_else(|| {
            format!(
                "terminal {} has finished and its output is no longer kept",
                term.id
            )
        })?;
        Ok(json!({ "text": screen.text(p.lines.min(100_000)) }))
    }

    fn send(&self, params: Value) -> Result<Value, String> {
        let p: SendParams = self::params(params)?;
        let term = self.shared.terminal(p.terminal)?;
        if !term.st.locked().info.state.is_running() {
            return Err(format!("terminal {} has finished", term.id));
        }
        let mut cmds = Vec::new();
        if !p.text.is_empty() {
            cmds.push(InputCmd::Paste(p.text.into_bytes()));
        }
        if p.submit {
            if !cmds.is_empty() {
                cmds.push(InputCmd::Gap(SUBMIT_GAP));
            }
            cmds.push(InputCmd::Bytes(b"\r".to_vec()));
        }
        if term.input.push(cmds) {
            Ok(Value::Null)
        } else {
            Err(format!(
                "terminal {} is not reading its input; too much is queued",
                term.id
            ))
        }
    }

    /// Ends the terminal on a thread of its own, so this client's other
    /// requests are not held up for the seconds a stubborn program takes.
    fn kill(
        &self,
        request: u64,
        term: Arc<Terminal>,
        remove: bool,
    ) -> Option<Result<Value, String>> {
        let shared = Arc::clone(&self.shared);
        let out = Arc::clone(&self.out);
        let for_error = Arc::clone(&self.out);
        let started = self.shared.spawn("kill", move || {
            let outcome = if !terminate(std::slice::from_ref(&term)) {
                Err(format!("terminal {} did not die", term.id))
            } else if remove {
                remove_terminal(&shared, &term).map(|()| Value::Null)
            } else {
                result(&term.st.locked().info)
            };
            reply(&out, request, outcome);
        });
        if let Err(e) = started {
            reply(
                &for_error,
                request,
                Err(format!("could not start a thread: {e}")),
            );
        }
        None
    }

    fn remove(&self, request: u64, p: &RemoveParams) -> Option<Result<Value, String>> {
        let term = match self.shared.terminal(p.terminal) {
            Ok(term) => term,
            Err(e) => return Some(Err(e)),
        };
        if term.st.locked().info.state.is_running() {
            if !p.force {
                return Some(Err(format!(
                    "terminal {} is running; kill it first or force",
                    term.id
                )));
            }
            return self.kill(request, term, true);
        }
        Some(remove_terminal(&self.shared, &term).map(|()| Value::Null))
    }

    fn rename(&self, params: Value) -> Result<Value, String> {
        let p: RenameParams = self::params(params)?;
        let term = self.shared.terminal(p.terminal)?;
        let mut st = term.st.locked();
        if st.info.name != p.name {
            st.info.name.clone_from(&p.name);
            self.shared.publish(&Event::Renamed {
                terminal: term.id,
                name: p.name,
            });
            self.shared.touch();
        }
        result(&st.info)
    }

    fn label(&self, params: Value) -> Result<Value, String> {
        let p: LabelParams = self::params(params)?;
        let term = self.shared.terminal(p.terminal)?;
        let mut st = term.st.locked();
        let changed = match p.value {
            Some(value) => st.info.labels.insert(p.key, value.clone()) != Some(value),
            None => st.info.labels.remove(&p.key).is_some(),
        };
        if changed {
            self.shared.publish(&Event::Labels {
                terminal: term.id,
                labels: st.info.labels.clone(),
            });
            self.shared.touch();
        }
        result(&st.info)
    }

    fn shutdown(&self, params: Value) -> Result<Value, String> {
        let p: ShutdownParams = self::params(params)?;
        let mut registry = self.shared.registry.locked();
        if !p.force {
            // Running terminals are counted with the registry closed to new
            // ones, so none can start between the count and the stop.
            let terms: Vec<_> = registry.terms.values().map(Arc::clone).collect();
            registry.closed = true;
            drop(registry);
            let running = terms
                .iter()
                .filter(|t| t.st.locked().info.state.is_running())
                .count();
            if running > 0 {
                self.shared.registry.locked().closed = false;
                return Err(format!(
                    "{running} terminal{} still running; force to end {}",
                    if running == 1 { "" } else { "s" },
                    if running == 1 { "it" } else { "them" },
                ));
            }
        } else {
            registry.closed = true;
            drop(registry);
        }
        log!("shutdown requested by client {}", self.id);
        self.shared.request_stop();
        Ok(Value::Null)
    }
}

fn remove_terminal(shared: &Shared, term: &Terminal) -> Result<(), String> {
    if shared.registry.locked().terms.remove(&term.id).is_none() {
        return Err(format!("no terminal {}", term.id));
    }
    let _st = term.st.locked();
    shared.publish(&Event::Removed { terminal: term.id });
    shared.touch();
    Ok(())
}

/// The environment of a new terminal's program.
fn child_env(
    shared: &Shared,
    id: u32,
    cwd: &str,
    extra: &BTreeMap<String, String>,
) -> Vec<(std::ffi::OsString, std::ffi::OsString)> {
    let mut env = crate::env::scrubbed();
    let has_locale = env
        .iter()
        .any(|(k, v)| (k == "LANG" || k == "LC_ALL") && !v.is_empty());
    let mut set = |key: &str, value: &std::ffi::OsStr| {
        env.retain(|(k, _)| k != key);
        env.push((key.into(), value.to_owned()));
    };
    set("TERM", "xterm-256color".as_ref());
    set("COLORTERM", "truecolor".as_ref());
    set("TERM_PROGRAM", "artalde".as_ref());
    if !has_locale {
        set("LANG", "en_US.UTF-8".as_ref());
    }
    set("PWD", cwd.as_ref());
    for (key, value) in extra {
        if !key.is_empty() && !key.contains('=') && !key.contains('\0') && !value.contains('\0') {
            set(key, value.as_ref());
        }
    }
    set("ARTALDE_TERMINAL", id.to_string().as_ref());
    set("ARTALDE_HOST_SOCKET", shared.socket.as_os_str());
    env
}

fn default_argv() -> Vec<String> {
    let shell = std::env::var("SHELL")
        .ok()
        .filter(|s| s.starts_with('/'))
        .unwrap_or_else(|| "/bin/sh".to_owned());
    vec![shell, "-l".to_owned()]
}

fn spawn_terminal(shared: &Arc<Shared>, p: RunParams) -> Result<TerminalInfo, String> {
    let argv = if p.argv.is_empty() {
        default_argv()
    } else {
        p.argv
    };
    if argv.iter().any(|a| a.contains('\0')) || argv[0].is_empty() {
        return Err("bad argv".to_owned());
    }
    let cwd = p
        .cwd
        .filter(|c| !c.is_empty())
        .or_else(|| std::env::var("HOME").ok())
        .unwrap_or_else(|| "/".to_owned());
    if !Path::new(&cwd).is_dir() {
        return Err(format!("{cwd} is not a directory"));
    }
    let (cols, rows) = (p.cols.max(1), p.rows.max(1));
    let name = p.name.filter(|n| !n.is_empty()).unwrap_or_else(|| {
        Path::new(&argv[0])
            .file_name()
            .map_or_else(|| argv[0].clone(), |n| n.to_string_lossy().into_owned())
    });
    let running = shared
        .terminals()
        .iter()
        .filter(|t| t.st.locked().info.state.is_running())
        .count();
    if running >= MAX_RUNNING {
        return Err(format!("{running} terminals are running already"));
    }

    let (gone_r, gone_w) = io::pipe().map_err(|e| format!("pipe: {e}"))?;
    let gone_for_input = gone_r.try_clone().map_err(|e| format!("pipe: {e}"))?;

    let mut spawner = shared.spawner.locked();
    let id = spawner.next;
    if id >= spawner.reserved {
        let reserved = id.saturating_add(ID_BLOCK);
        paths::write_atomic(
            &paths::seq_file(&shared.state_dir),
            reserved.to_string().as_bytes(),
        )
        .map_err(|e| format!("could not reserve terminal ids: {e}"))?;
        spawner.reserved = reserved;
    }
    spawner.next = id.checked_add(1).ok_or("out of terminal ids")?;
    let pty = Pty::open(cols, rows).map_err(|e| format!("could not open a terminal: {e}"))?;
    let [stdin, stdout, stderr] = pty.stdio().map_err(|e| format!("terminal: {e}"))?;
    let mut cmd = Command::new(&argv[0]);
    cmd.args(&argv[1..])
        .env_clear()
        .envs(child_env(shared, id, &cwd, &p.env))
        .current_dir(&cwd)
        .stdin(stdin)
        .stdout(stdout)
        .stderr(stderr);
    crate::sys::new_session(&mut cmd, true);
    let child = cmd
        .spawn()
        .map_err(|e| format!("could not run {}: {e}", argv[0]))?;
    drop(spawner);
    // The command holds the child's copies of the slave end.
    drop(cmd);

    let pid = Pid::from_child(&child);
    let pty = Arc::new(pty);
    let info = TerminalInfo {
        id,
        name,
        argv,
        cwd,
        pid: Some(child.id()),
        cols,
        rows,
        created: now(),
        state: State::Running,
        title: None,
        status: None,
        labels: p.labels,
        foreground: None,
        viewers: 0,
    };
    let term = Arc::new(Terminal {
        id,
        st: Mutex::new(TermState {
            info: info.clone(),
            screen: Some(Screen::new(cols, rows)),
            pty: Some(Arc::clone(&pty)),
            child: Some(child),
            reaped: false,
            exit: None,
            gone: Some(gone_w),
            viewers: Vec::new(),
            leader: None,
            turn: 0,
            foreground: None,
            cwd_reported: false,
        }),
        input: InputQueue::default(),
    });

    let abort = |why: String| {
        kill_now(&term);
        reap(&term, pid);
        term.input.close();
        why
    };
    {
        let mut registry = shared.registry.locked();
        if registry.closed {
            drop(registry);
            return Err(abort("the host is stopping".to_owned()));
        }
        registry.terms.insert(id, Arc::clone(&term));
    }
    // Before any thread of the terminal runs: `created` is the first event
    // about it.
    {
        let _st = term.st.locked();
        shared.publish(&Event::Created {
            terminal: id,
            info: Box::new(info.clone()),
        });
    }
    shared.touch();

    let (for_reader, shared_reader, pty_reader) =
        (Arc::clone(&term), Arc::clone(shared), Arc::clone(&pty));
    if let Err(e) = shared.spawn("pty reader", move || {
        read_loop(&shared_reader, &for_reader, &pty_reader, &gone_r);
    }) {
        shared.registry.locked().terms.remove(&id);
        let why = abort(format!("could not start a thread: {e}"));
        let _st = term.st.locked();
        shared.publish(&Event::Removed { terminal: id });
        return Err(why);
    }
    let for_waiter = Arc::clone(&term);
    if let Err(e) = shared.spawn("child waiter", move || reap(&for_waiter, pid)) {
        log!("terminal {id}: waiter thread: {e}");
        kill_now(&term);
        reap(&term, pid);
        return Err(format!("could not start a thread: {e}"));
    }
    let (for_input, shared_input) = (Arc::clone(&term), Arc::clone(shared));
    if let Err(e) = shared.spawn("pty writer", move || {
        input_loop(&shared_input, &for_input, &pty, &gone_for_input);
    }) {
        log!("terminal {id}: input thread: {e}");
        term.input.close();
    }
    Ok(info)
}

/// `SIGKILL` to the terminal's process group, unless it is reaped already.
fn kill_now(term: &Terminal) {
    signal(&term.st.locked(), Signal::KILL);
}

/// Signals the terminal's process group and, when another group is in the
/// foreground, that one too. Called with the terminal locked: the lock is
/// what keeps the child from being reaped, and its pid reused, in between.
fn signal(st: &TermState, signal: Signal) {
    if st.reaped {
        return;
    }
    let Some(child) = &st.child else { return };
    let group = Pid::from_child(child);
    rustix::process::kill_process_group(group, signal).ok();
    if let Some(foreground) = st.pty.as_ref().and_then(|p| p.foreground())
        && foreground != group
        && foreground.as_raw_nonzero().get() > 1
    {
        rustix::process::kill_process_group(foreground, signal).ok();
    }
}

/// Waits for the child and records how it ended. The child stays a zombie,
/// its pid taken, until the terminal is locked and marked reaped.
fn reap(term: &Terminal, pid: Pid) {
    loop {
        match rustix::process::waitid(
            WaitId::Pid(pid),
            WaitIdOptions::EXITED | WaitIdOptions::NOWAIT,
        ) {
            Err(Errno::INTR) => {}
            Ok(_) => break,
            Err(e) => {
                log!("terminal {}: waitid: {e}", term.id);
                break;
            }
        }
    }
    let mut st = term.st.locked();
    if st.reaped {
        return;
    }
    // Returns at once when `waitid` succeeded. When it did not, this blocks
    // under the lock, which only delays a `kill` that could do nothing.
    let status = st.child.as_mut().and_then(|c| c.wait().ok());
    st.reaped = true;
    st.exit = Some(Ended {
        code: status.and_then(|s| s.code()),
        signal: status.and_then(|s| s.signal()),
    });
    st.gone = None;
}

fn poll_timeout(left: Duration) -> Timespec {
    Timespec {
        tv_sec: i64::try_from(left.as_secs()).unwrap_or(i64::MAX),
        tv_nsec: left.subsec_nanos().into(),
    }
}

/// The PTY reader: reads whenever there is something, so the program never
/// blocks on a full terminal because of a slow viewer.
fn read_loop(shared: &Shared, term: &Terminal, pty: &Pty, gone: &PipeReader) {
    let master = pty.master();
    let mut buf = vec![0u8; READ_CHUNK];
    // Set once the child is gone: read what is left, then stop.
    let mut deadline: Option<Instant> = None;
    'read: loop {
        loop {
            match rustix::io::read(master, &mut buf) {
                Ok(0) => break 'read,
                Ok(n) => on_output(shared, term, &buf[..n]),
                Err(Errno::INTR) => {}
                Err(Errno::AGAIN) => break,
                // `EIO`: every slave end is closed.
                Err(_) => break 'read,
            }
            if deadline.is_some_and(|d| Instant::now() >= d) {
                break 'read;
            }
        }
        match deadline {
            None => {
                let mut fds = [
                    PollFd::new(&master, PollFlags::IN),
                    PollFd::new(gone, PollFlags::IN),
                ];
                match rustix::event::poll(&mut fds, None) {
                    Ok(_) | Err(Errno::INTR) => {}
                    Err(e) => {
                        log!("terminal {}: poll: {e}", term.id);
                        break 'read;
                    }
                }
                if !fds[1].revents().is_empty() {
                    deadline = Some(Instant::now() + DRAIN_GRACE);
                }
            }
            Some(deadline) => {
                // The child's last output is in the PTY already; one short
                // wait covers a kernel that is still moving it across.
                let left = deadline
                    .saturating_duration_since(Instant::now())
                    .min(Duration::from_millis(20));
                let mut fds = [PollFd::new(&master, PollFlags::IN)];
                let ready = rustix::event::poll(&mut fds, Some(&poll_timeout(left)));
                if !matches!(ready, Ok(n) if n > 0) || left.is_zero() {
                    break 'read;
                }
            }
        }
    }
    if deadline.is_none() {
        // The PTY failed before the child went; its exit is still what ends
        // the terminal.
        let mut fds = [PollFd::new(gone, PollFlags::IN)];
        while matches!(rustix::event::poll(&mut fds, None), Err(Errno::INTR)) {}
    }
    finish(shared, term);
}

fn on_output(shared: &Shared, term: &Terminal, bytes: &[u8]) {
    let mut st = term.st.locked();
    let Some(screen) = &mut st.screen else { return };
    let events = screen.feed(bytes);
    let replies = screen.take_replies();
    for event in events {
        match event {
            ScreenEvent::Title(title) => {
                let title = (!title.is_empty()).then_some(title);
                if st.info.title != title {
                    st.info.title.clone_from(&title);
                    shared.publish(&Event::Title {
                        terminal: term.id,
                        title,
                    });
                    shared.touch();
                }
            }
            ScreenEvent::Cwd(cwd) => {
                st.cwd_reported = true;
                if st.info.cwd != cwd {
                    st.info.cwd.clone_from(&cwd);
                    shared.publish(&Event::Cwd {
                        terminal: term.id,
                        cwd,
                    });
                    shared.touch();
                }
            }
            ScreenEvent::Bell => shared.publish(&Event::Bell { terminal: term.id }),
            ScreenEvent::Status(report) => st.set_status(Some(report), shared),
        }
    }
    if !replies.is_empty() {
        term.input.push(vec![InputCmd::Bytes(replies)]);
    }
    let TermState {
        viewers, screen, ..
    } = &mut *st;
    viewers.retain(
        |viewer| match viewer.out.queue.push_output(term.id, bytes, VIEWER_BACKLOG) {
            Push::Queued => true,
            Push::Overflow => {
                // Too far behind to catch up byte by byte: what it has not
                // been sent is replaced by the screen as it is now.
                if let Some(screen) = screen {
                    viewer
                        .out
                        .queue
                        .replace_output(term.id, screen.replay(viewer.history));
                }
                true
            }
            Push::Closed => false,
        },
    );
    st.info.viewers = u32::try_from(st.viewers.len()).unwrap_or(u32::MAX);
}

/// The terminal has ended and all its output is in: tell everyone.
fn finish(shared: &Shared, term: &Terminal) {
    let mut st = term.st.locked();
    let Ended { code, signal } = st.exit.unwrap_or(Ended {
        code: None,
        signal: None,
    });
    let at = now();
    st.info.state = State::Exited {
        code,
        signal,
        at,
        lost: false,
    };
    st.info.pid = None;
    st.info.foreground = None;
    st.foreground = None;
    st.set_status(None, shared);
    for viewer in st.viewers.drain(..) {
        viewer
            .out
            .send(Frame::json(Kind::Ended, term.id, &Ended { code, signal }));
    }
    st.info.viewers = 0;
    st.leader = None;
    shared.publish(&Event::Exited {
        terminal: term.id,
        code,
        signal,
        at,
    });
    st.pty = None;
    st.child = None;
    drop(st);
    term.input.close();
    shared.touch();
}

/// Writes to the PTY, waiting while the program is not reading. `false`
/// when the terminal ended first.
fn write_input(pty: &Pty, gone: &PipeReader, mut bytes: &[u8]) -> bool {
    let master = pty.master();
    while !bytes.is_empty() {
        match rustix::io::write(master, bytes) {
            Ok(n) => bytes = &bytes[n..],
            Err(Errno::INTR) => {}
            Err(Errno::AGAIN) => {
                let mut fds = [
                    PollFd::new(&master, PollFlags::OUT),
                    PollFd::new(gone, PollFlags::IN),
                ];
                match rustix::event::poll(&mut fds, None) {
                    Ok(_) | Err(Errno::INTR) => {}
                    Err(_) => return false,
                }
                if !fds[1].revents().is_empty() {
                    return false;
                }
            }
            Err(_) => return false,
        }
    }
    true
}

/// Sleeps unless the terminal ends first; `false` when it did.
fn pause(gone: &PipeReader, time: Duration) -> bool {
    let deadline = Instant::now() + time;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return true;
        }
        let mut fds = [PollFd::new(gone, PollFlags::IN)];
        match rustix::event::poll(&mut fds, Some(&poll_timeout(left))) {
            Ok(0) | Err(Errno::INTR) => {}
            Ok(_) | Err(_) => return false,
        }
    }
}

fn paste_bytes(text: &[u8], bracketed: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len() + 12);
    if bracketed {
        out.extend_from_slice(b"\x1b[200~");
    }
    let mut rest = text;
    while let Some((&b, tail)) = rest.split_first() {
        rest = tail;
        match b {
            // A terminal sends Enter as a carriage return, pasted or typed.
            b'\r' if rest.first() == Some(&b'\n') => {}
            b'\n' => out.push(b'\r'),
            // Nothing pasted may end the paste early, or carry a sequence
            // of its own.
            0x1b => {}
            _ => out.push(b),
        }
    }
    if bracketed {
        out.extend_from_slice(b"\x1b[201~");
    }
    out
}

fn input_loop(shared: &Shared, term: &Terminal, pty: &Pty, gone: &PipeReader) {
    while let Some(cmd) = term.input.pop() {
        let alive = match cmd {
            InputCmd::Bytes(bytes) => write_input(pty, gone, &bytes),
            InputCmd::Paste(text) => {
                let bracketed = term
                    .st
                    .locked()
                    .screen
                    .as_ref()
                    .is_some_and(Screen::bracketed_paste);
                write_input(pty, gone, &paste_bytes(&text, bracketed))
            }
            InputCmd::Gap(time) => pause(gone, time),
            InputCmd::Nudge => nudge(shared, term, pty, gone),
        };
        if !alive {
            break;
        }
    }
    term.input.close();
}

/// One row fewer and back: two size changes, of which a full-screen program
/// repaints at least the second.
fn nudge(_shared: &Shared, term: &Terminal, pty: &Pty, gone: &PipeReader) -> bool {
    {
        let st = term.st.locked();
        if st.info.rows < 2 || !st.info.state.is_running() {
            return true;
        }
        pty.resize(st.info.cols, st.info.rows - 1, 0, 0).ok();
    }
    let alive = pause(gone, NUDGE_GAP);
    let st = term.st.locked();
    // Whatever the size is by now, which a viewer may have changed.
    pty.resize(st.info.cols, st.info.rows, 0, 0).ok();
    alive
}

/// Ends the given terminals: `HUP`, then `TERM`, then `KILL`, each given
/// time to work. `true` when all of them are gone.
fn terminate(terms: &[Arc<Terminal>]) -> bool {
    let running = |term: &Arc<Terminal>| term.st.locked().info.state.is_running();
    for (sig, wait) in [
        (Signal::HUP, KILL_STEP),
        (Signal::TERM, KILL_STEP),
        (Signal::KILL, KILL_LAST),
    ] {
        let mut any = false;
        for term in terms {
            let st = term.st.locked();
            if st.info.state.is_running() {
                any = true;
                signal(&st, sig);
            }
        }
        if !any {
            return true;
        }
        let deadline = Instant::now() + wait;
        while Instant::now() < deadline {
            if !terms.iter().any(running) {
                return true;
            }
            thread::sleep(Duration::from_millis(5));
        }
    }
    !terms.iter().any(running)
}

fn raise_nofile() {
    use rustix::process::{Resource, Rlimit, getrlimit, setrlimit};

    let limit = getrlimit(Resource::Nofile);
    // macOS refuses a soft limit over `OPEN_MAX` even when the hard limit
    // says unlimited.
    let want = limit.maximum.map_or(10_240, |max| max.min(65_536));
    if limit.current.is_some_and(|current| current >= want) {
        return;
    }
    for target in [want, want.min(10_240)] {
        let raised = Rlimit {
            current: Some(target),
            maximum: limit.maximum,
        };
        if setrlimit(Resource::Nofile, raised).is_ok() {
            return;
        }
    }
}

fn keeper_loop(shared: &Arc<Shared>) {
    let mut last_save = Instant::now();
    let mut tick = 0u64;
    loop {
        {
            let phase = shared.phase.locked();
            let phase = if *phase == Phase::Running {
                shared
                    .phase_changed
                    .wait_timeout(phase, TICK)
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .0
            } else {
                phase
            };
            if *phase != Phase::Running {
                break;
            }
        }
        tick += 1;
        if tick.is_multiple_of(2) {
            poll_foreground(shared);
        }
        if tick.is_multiple_of(8) {
            prune(shared);
            reap_threads(shared);
        }
        if shared.dirty.load(Ordering::Relaxed) && last_save.elapsed() >= SAVE_DEBOUNCE {
            shared.save();
            last_save = Instant::now();
        }
    }
    teardown(shared);
}

/// Joins the threads that have ended.
fn reap_threads(shared: &Shared) {
    let finished: Vec<_> = {
        let mut threads = shared.threads.locked();
        let (finished, running) = std::mem::take(&mut *threads)
            .into_iter()
            .partition(JoinHandle::is_finished);
        *threads = running;
        finished
    };
    for handle in finished {
        handle.join().ok();
    }
}

/// Which program is in the foreground of each terminal, and where its shell
/// is.
fn poll_foreground(shared: &Shared) {
    for term in shared.terminals() {
        let (pty, root, cwd_reported) = {
            let st = term.st.locked();
            let (Some(pty), Some(child), false) = (&st.pty, &st.child, st.reaped) else {
                continue;
            };
            (Arc::clone(pty), Pid::from_child(child), st.cwd_reported)
        };
        // The lookups are system calls about other processes; none of it
        // under the lock. A pid that was reused in between gives a wrong
        // name for half a second at worst.
        let foreground = pty.foreground();
        let name = foreground.and_then(procinfo::name);
        let cwd = (!cwd_reported).then(|| procinfo::cwd(root)).flatten();
        let mut st = term.st.locked();
        if !st.info.state.is_running() || st.reaped {
            continue;
        }
        let back_at_root =
            foreground == Some(root) && st.foreground.is_some() && st.foreground != Some(root);
        st.foreground = foreground;
        if st.info.foreground != name {
            st.info.foreground.clone_from(&name);
            shared.publish(&Event::Foreground {
                terminal: term.id,
                name,
            });
        }
        if let Some(cwd) = cwd
            && !st.cwd_reported
            && st.info.cwd != cwd
        {
            st.info.cwd.clone_from(&cwd);
            shared.publish(&Event::Cwd {
                terminal: term.id,
                cwd,
            });
            shared.touch();
        }
        // The program that reported a status has handed the terminal back
        // to the shell: whatever it said no longer holds.
        if back_at_root && st.info.status.is_some() {
            st.set_status(None, shared);
        }
    }
}

/// Forgets finished terminals that are old or too many, and the output of
/// all but the latest few.
fn prune(shared: &Shared) {
    let mut finished: Vec<(u64, Arc<Terminal>)> = shared
        .terminals()
        .into_iter()
        .filter_map(|term| {
            let at = match term.st.locked().info.state {
                State::Exited { at, .. } => at,
                State::Running => return None,
            };
            Some((at, term))
        })
        .collect();
    // Latest first.
    finished.sort_by_key(|(at, term)| (std::cmp::Reverse(*at), std::cmp::Reverse(term.id)));
    let cutoff = now().saturating_sub(KEEP_FINISHED_SECS);
    for (rank, (at, term)) in finished.iter().enumerate() {
        if rank >= KEEP_FINISHED || *at < cutoff {
            remove_terminal(shared, term).ok();
        } else if rank >= KEEP_SCREENS {
            term.st.locked().screen = None;
        }
    }
}

fn teardown(shared: &Shared) {
    *shared.accept_stop.locked() = None;
    let terms = {
        let mut registry = shared.registry.locked();
        registry.closed = true;
        registry.terms.values().map(Arc::clone).collect::<Vec<_>>()
    };
    if !terminate(&terms) {
        log!("some terminals did not die");
    }

    // Let clients have what is queued for them (the reply to `shutdown`,
    // the last output), within reason.
    let clients: Vec<_> = shared.clients.locked().values().map(Arc::clone).collect();
    let deadline = Instant::now() + Duration::from_secs(1);
    for client in &clients {
        client.queue.close();
    }
    for client in &clients {
        client
            .queue
            .wait_written(deadline.saturating_duration_since(Instant::now()));
        client.kick();
    }
    drop(clients);

    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        reap_threads(shared);
        let left = shared.threads.locked().len();
        if left == 0 {
            break;
        }
        if Instant::now() >= deadline {
            log!("{left} threads did not end");
            break;
        }
        thread::sleep(Duration::from_millis(2));
        // A client that connected while the others were being closed.
        for client in shared.clients.locked().values() {
            client.kick();
        }
    }

    shared.save();
    fs::remove_file(&shared.socket).ok();
    fs::remove_file(paths::addr_file(&shared.state_dir)).ok();
    *shared.lock.locked() = None;
    log!("stopped");
    *shared.phase.locked() = Phase::Stopped;
    shared.phase_changed.notify_all();
}

impl std::fmt::Debug for Conn {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Conn").field("id", &self.id).finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paste_is_one_line_per_return_and_cannot_escape() {
        assert_eq!(paste_bytes(b"a\nb\r\nc", false), b"a\rb\rc");
        assert_eq!(
            paste_bytes(b"x\x1b[201~y", true),
            b"\x1b[200~x[201~y\x1b[201~"
        );
    }

    #[test]
    fn input_queue_is_bounded_and_all_or_nothing() {
        let q = InputQueue::default();
        assert!(q.push(vec![InputCmd::Bytes(vec![0; INPUT_BACKLOG / 2])]));
        assert!(!q.push(vec![
            InputCmd::Bytes(vec![0; 16]),
            InputCmd::Bytes(vec![0; INPUT_BACKLOG / 2]),
        ]));
        assert!(matches!(q.pop(), Some(InputCmd::Bytes(_))));
        assert!(q.push(vec![InputCmd::Gap(SUBMIT_GAP)]));
        q.close();
        assert!(q.pop().is_none());
        assert!(!q.push(vec![InputCmd::Nudge]));
    }
}
