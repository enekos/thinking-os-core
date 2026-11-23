//! The client side of the wire protocol.
//!
//! A [`Client`] owns one connection and two threads: a reader, which sorts
//! what the host sends into the waiting request, the attached [`Stream`] or
//! the [`Events`] feed, and a writer. Nothing here blocks the caller for
//! longer than a request's timeout, and the reader never blocks on the
//! caller: what a slow consumer does not take is bounded and dropped in a
//! way it can recover from.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::fmt;
use std::fs::{self, File};
use std::io;
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use rustix::fs::FlockOperation;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::paths;
use crate::queue::{FrameQueue, Push};
use crate::util::LockExt as _;
use crate::wire::{
    Ended, Event, Frame, HostInfo, Kind, PROTOCOL_VERSION, TerminalInfo, encode_resize, read_frame,
};

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);
const SPAWN_TIMEOUT: Duration = Duration::from_secs(10);
/// Output buffered for a stream nobody reads before it is dropped and the
/// stream re-attached.
const STREAM_BUFFER: usize = 8 << 20;
/// Input and requests queued for a host that is not reading.
const SEND_BUFFER: usize = 8 << 20;
const EVENT_BUFFER: usize = 8192;
const INPUT_CHUNK: usize = 32 << 10;
const LOG_ROTATE_BYTES: u64 = 4 << 20;

/// What to do when no host is running.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Spawn {
    /// Start `<exe> host` detached and wait for it.
    IfNeeded { exe: PathBuf },
    /// Fail with [`ClientError::NotRunning`].
    Never,
}

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ClientError {
    /// The connection to the host is gone. [`Client::reconnect`] makes a new
    /// one.
    #[error("disconnected from the terminal host")]
    Disconnected,
    #[error("no terminal host is running")]
    NotRunning,
    #[error("the terminal host did not answer in time")]
    Timeout,
    /// The host refused the request.
    #[error("{0}")]
    Host(String),
    #[error("could not start the terminal host: {0}")]
    Spawn(String),
    /// More is queued for the host than it takes.
    #[error("the terminal host is not keeping up")]
    Busy,
    #[error("the terminal host sent something unexpected: {0}")]
    Protocol(String),
    #[error(transparent)]
    Io(#[from] io::Error),
}

/// What to run in a new terminal. Empty `argv` is the user's login shell.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RunOptions {
    pub argv: Vec<String>,
    pub cwd: Option<String>,
    pub env: BTreeMap<String, String>,
    pub name: Option<String>,
    pub cols: u16,
    pub rows: u16,
    pub labels: BTreeMap<String, String>,
}

impl Default for RunOptions {
    fn default() -> RunOptions {
        RunOptions {
            argv: Vec::new(),
            cwd: None,
            env: BTreeMap::new(),
            name: None,
            cols: 80,
            rows: 24,
            labels: BTreeMap::new(),
        }
    }
}

/// How a [`Stream`] ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Exit {
    /// The program is gone. Both `None` when it ended some way the host did
    /// not see.
    Exited {
        code: Option<i32>,
        signal: Option<i32>,
    },
    /// The connection went first; the program may still run.
    Disconnected,
    /// The host stopped sending for the reason given.
    Failed(String),
}

type Waker = Arc<dyn Fn() + Send + Sync>;

/// Tells the owner that something arrived, once per [`Client::woken`].
#[derive(Default)]
struct Wake {
    waker: Mutex<Option<Waker>>,
    woken: AtomicBool,
}

impl Wake {
    fn notify(&self) {
        if !self.woken.swap(true, Ordering::AcqRel) {
            let waker = self.waker.locked().as_ref().map(Arc::clone);
            if let Some(waker) = waker {
                waker();
            }
        }
    }
}

impl fmt::Debug for Wake {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Wake")
            .field("woken", &self.woken)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Default)]
struct StreamBuf {
    chunks: VecDeque<Vec<u8>>,
    bytes: usize,
    ended: Option<Exit>,
    /// The buffer overflowed and was dropped; the stream is detached until
    /// its reader comes back and it is attached again.
    lagged: bool,
    reattaching: bool,
    cols: u16,
    rows: u16,
    history: bool,
}

#[derive(Debug, Default)]
struct StreamShared {
    buf: Mutex<StreamBuf>,
}

impl StreamShared {
    fn end(&self, exit: Exit) {
        let mut buf = self.buf.locked();
        if buf.ended.is_none() {
            buf.ended = Some(exit);
        }
    }
}

#[derive(Debug, Default)]
struct EventsShared {
    queue: Mutex<VecDeque<Event>>,
    lagged: AtomicBool,
}

enum Pending {
    Call(SyncSender<Result<Value, ClientError>>),
    Attach {
        terminal: u32,
        stream: Arc<StreamShared>,
        /// `None` when the stream re-attaches itself after falling behind.
        reply: Option<SyncSender<Result<Value, ClientError>>>,
    },
}

#[derive(Default)]
struct LinkState {
    pending: HashMap<u64, Pending>,
    streams: HashMap<u32, Arc<StreamShared>>,
    events: Option<Arc<EventsShared>>,
    dead: bool,
}

/// One connection.
struct Link {
    queue: FrameQueue,
    socket: UnixStream,
    connected: AtomicBool,
    next_id: AtomicU64,
    state: Mutex<LinkState>,
    wake: Arc<Wake>,
}

impl fmt::Debug for Link {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Link")
            .field("connected", &self.connected)
            .finish_non_exhaustive()
    }
}

#[derive(Deserialize)]
struct Reply {
    id: u64,
    #[serde(default)]
    result: Value,
    #[serde(default)]
    error: Option<String>,
}

impl Link {
    fn is_connected(&self) -> bool {
        self.connected.load(Ordering::Acquire)
    }

    fn close(&self) {
        self.connected.store(false, Ordering::Release);
        self.queue.abort();
        self.socket.shutdown(Shutdown::Both).ok();
    }

    /// Queues a request and registers who waits for its reply.
    fn request(&self, method: &str, params: &Value, pending: Pending) -> Result<u64, ClientError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let frame = Frame::json(
            Kind::Request,
            0,
            &json!({ "id": id, "method": method, "params": params }),
        );
        {
            let mut state = self.state.locked();
            if state.dead {
                return Err(ClientError::Disconnected);
            }
            state.pending.insert(id, pending);
        }
        match self.queue.push_within(frame, SEND_BUFFER) {
            Push::Queued => Ok(id),
            refused => {
                self.state.locked().pending.remove(&id);
                Err(match refused {
                    Push::Overflow => ClientError::Busy,
                    _ => ClientError::Disconnected,
                })
            }
        }
    }

    fn send(&self, frame: Frame) -> Result<(), ClientError> {
        match self.queue.push_within(frame, SEND_BUFFER) {
            Push::Queued => Ok(()),
            Push::Overflow => Err(ClientError::Busy),
            Push::Closed => Err(ClientError::Disconnected),
        }
    }

    /// The reader thread.
    fn read(&self, mut socket: UnixStream) {
        loop {
            match read_frame(&mut socket) {
                Ok(Some(frame)) => {
                    if let Err(why) = self.incoming(frame) {
                        crate::util::log!("client: {why}");
                        break;
                    }
                }
                Ok(None) => break,
                Err(e) => {
                    if self.is_connected() {
                        crate::util::log!("client: {e}");
                    }
                    break;
                }
            }
        }
        self.close();
        let mut state = self.state.locked();
        state.dead = true;
        // Dropping the senders is what tells the callers.
        state.pending.clear();
        for (_, stream) in state.streams.drain() {
            stream.end(Exit::Disconnected);
        }
        drop(state);
        self.wake.notify();
    }

    fn incoming(&self, frame: Frame) -> Result<(), String> {
        match frame.kind {
            Kind::Reply => {
                let reply: Reply = serde_json::from_slice(&frame.payload)
                    .map_err(|e| format!("bad reply: {e}"))?;
                self.reply(reply);
            }
            Kind::Event => {
                let events = self.state.locked().events.as_ref().map(Arc::clone);
                // An event of a kind this build does not know is skipped.
                if let Some(events) = events
                    && let Ok(event) = serde_json::from_slice::<Event>(&frame.payload)
                {
                    let mut queue = events.queue.locked();
                    if queue.len() >= EVENT_BUFFER {
                        events.lagged.store(true, Ordering::Release);
                    } else {
                        queue.push_back(event);
                    }
                    drop(queue);
                    self.wake.notify();
                }
            }
            Kind::Output => {
                let stream = self
                    .state
                    .locked()
                    .streams
                    .get(&frame.terminal)
                    .map(Arc::clone);
                if let Some(stream) = stream {
                    let mut buf = stream.buf.locked();
                    if buf.lagged {
                        // Sent before the host saw the detach.
                    } else if buf.bytes + frame.payload.len() > STREAM_BUFFER {
                        buf.chunks.clear();
                        buf.bytes = 0;
                        buf.lagged = true;
                        self.queue
                            .push(Frame::new(Kind::Detach, frame.terminal, Vec::new()));
                    } else {
                        buf.bytes += frame.payload.len();
                        buf.chunks.push_back(frame.payload);
                    }
                    drop(buf);
                    self.wake.notify();
                }
            }
            Kind::Ended => {
                let ended: Ended =
                    serde_json::from_slice(&frame.payload).map_err(|e| format!("bad end: {e}"))?;
                if let Some(stream) = self.state.locked().streams.remove(&frame.terminal) {
                    stream.end(Exit::Exited {
                        code: ended.code,
                        signal: ended.signal,
                    });
                    self.wake.notify();
                }
            }
            Kind::Request | Kind::Input | Kind::Resize | Kind::Detach => {
                return Err(format!("the host sent a client frame ({:?})", frame.kind));
            }
        }
        Ok(())
    }

    fn reply(&self, reply: Reply) {
        let outcome = match reply.error {
            Some(error) => Err(ClientError::Host(error)),
            None => Ok(reply.result),
        };
        let mut state = self.state.locked();
        match state.pending.remove(&reply.id) {
            Some(Pending::Call(tx)) => {
                drop(state);
                tx.try_send(outcome).ok();
            }
            Some(Pending::Attach {
                terminal,
                stream,
                reply,
            }) => {
                // The stream is in place before the next frame is read, and
                // the next frame for it is the start of the replay.
                match &outcome {
                    Ok(_) => {
                        let mut buf = stream.buf.locked();
                        buf.chunks.clear();
                        buf.bytes = 0;
                        buf.lagged = false;
                        buf.reattaching = false;
                        drop(buf);
                        if let Some(old) = state.streams.insert(terminal, Arc::clone(&stream))
                            && !Arc::ptr_eq(&old, &stream)
                        {
                            old.end(Exit::Failed("attached again".to_owned()));
                        }
                    }
                    Err(e) => {
                        if state
                            .streams
                            .get(&terminal)
                            .is_some_and(|s| Arc::ptr_eq(s, &stream))
                        {
                            state.streams.remove(&terminal);
                        }
                        stream.end(Exit::Failed(e.to_string()));
                    }
                }
                drop(state);
                match reply {
                    Some(tx) => {
                        tx.try_send(outcome).ok();
                    }
                    None => self.wake.notify(),
                }
            }
            // The caller gave up waiting.
            None => {}
        }
    }
}

/// A connection to the terminal host.
#[derive(Debug)]
pub struct Client {
    state_dir: PathBuf,
    spawn: Spawn,
    timeout: Duration,
    wake: Arc<Wake>,
    conn: Mutex<Conn>,
}

#[derive(Debug, Default)]
struct Conn {
    link: Option<Arc<Link>>,
    threads: Vec<JoinHandle<()>>,
    /// The host this client started, kept to collect its exit.
    host: Option<Child>,
    info: Option<HostInfo>,
}

impl Client {
    /// Connects to the host of `state_dir`, starting one when `spawn` says
    /// so. A host that speaks another protocol version is an error.
    pub fn connect(state_dir: &Path, spawn: Spawn) -> Result<Client, ClientError> {
        let client = Client {
            state_dir: state_dir.to_path_buf(),
            spawn,
            timeout: DEFAULT_TIMEOUT,
            wake: Arc::new(Wake::default()),
            conn: Mutex::new(Conn::default()),
        };
        client.reconnect()?;
        Ok(client)
    }

    /// How long a request waits for its reply. 10 s unless set.
    pub fn set_timeout(&mut self, timeout: Duration) {
        self.timeout = timeout;
    }

    /// Drops the connection, if any, and makes a new one, starting a host
    /// as [`Client::connect`] would. Streams and event feeds of the old
    /// connection end as disconnected; attach and subscribe again.
    pub fn reconnect(&self) -> Result<(), ClientError> {
        let mut conn = self.conn.locked();
        Self::hang_up(&mut conn);
        let socket = self.dial(&mut conn)?;
        let link = Arc::new(Link {
            queue: FrameQueue::default(),
            socket: socket.try_clone()?,
            connected: AtomicBool::new(true),
            next_id: AtomicU64::new(1),
            state: Mutex::new(LinkState::default()),
            wake: Arc::clone(&self.wake),
        });
        let write_half = socket.try_clone()?;
        let for_writer = Arc::clone(&link);
        let writer = thread::Builder::new()
            .name("artalde-host client writer".into())
            .spawn(move || {
                for_writer.queue.write_to(&write_half);
                for_writer.socket.shutdown(Shutdown::Both).ok();
            })?;
        conn.threads.push(writer);
        let for_reader = Arc::clone(&link);
        let reader = thread::Builder::new()
            .name("artalde-host client reader".into())
            .spawn(move || for_reader.read(socket));
        match reader {
            Ok(reader) => conn.threads.push(reader),
            Err(e) => {
                link.close();
                Self::hang_up(&mut conn);
                return Err(e.into());
            }
        }
        conn.link = Some(Arc::clone(&link));
        drop(conn);
        let hello = self.call_on::<HostInfo>(
            &link,
            "hello",
            &json!({ "client": "artalde", "version": PROTOCOL_VERSION }),
        );
        match hello {
            Ok(info) if info.version == PROTOCOL_VERSION => {
                self.conn.locked().info = Some(info);
                Ok(())
            }
            Ok(info) => {
                Self::hang_up(&mut self.conn.locked());
                Err(ClientError::Protocol(format!(
                    "it speaks protocol version {}, this client {PROTOCOL_VERSION}",
                    info.version
                )))
            }
            Err(e) => {
                Self::hang_up(&mut self.conn.locked());
                Err(e)
            }
        }
    }

    fn hang_up(conn: &mut Conn) {
        if let Some(link) = conn.link.take() {
            link.close();
        }
        for thread in conn.threads.drain(..) {
            thread.join().ok();
        }
        conn.info = None;
        if let Some(host) = &mut conn.host
            && matches!(host.try_wait(), Ok(Some(_)) | Err(_))
        {
            conn.host = None;
        }
    }

    fn lock_is_free(&self) -> bool {
        let Ok(lock) = File::open(paths::lock_file(&self.state_dir)) else {
            return true;
        };
        // Shared, so two clients asking at once both see it free.
        rustix::fs::flock(&lock, FlockOperation::NonBlockingLockShared).is_ok()
    }

    fn dial(&self, conn: &mut Conn) -> Result<UnixStream, ClientError> {
        let deadline = Instant::now() + SPAWN_TIMEOUT;
        let mut spawned = 0;
        loop {
            // Read again each time: the host writes the address it chose.
            let socket = paths::socket_path(&self.state_dir);
            if let Ok(stream) = UnixStream::connect(&socket) {
                return Ok(stream);
            }
            let Spawn::IfNeeded { exe } = &self.spawn else {
                return Err(ClientError::NotRunning);
            };
            let ours_exited = match &mut conn.host {
                Some(host) => !matches!(host.try_wait(), Ok(None)),
                None => true,
            };
            // The lock taken means a host is starting or stopping; either
            // way the thing to do is wait.
            if ours_exited && self.lock_is_free() {
                if spawned >= 3 {
                    return Err(ClientError::Spawn(format!(
                        "it keeps exiting; see {}",
                        paths::log_file(&self.state_dir).display()
                    )));
                }
                spawned += 1;
                conn.host = Some(self.spawn_host(exe)?);
            }
            if Instant::now() >= deadline {
                return Err(ClientError::Spawn(format!(
                    "it did not come up; see {}",
                    paths::log_file(&self.state_dir).display()
                )));
            }
            thread::sleep(Duration::from_millis(20));
        }
    }

    /// Starts `<exe> host` in a session of its own, with nothing of this
    /// process's terminal or agent session in its environment.
    fn spawn_host(&self, exe: &Path) -> Result<Child, ClientError> {
        let spawn_err = |what: &str| {
            let what = what.to_owned();
            move |e: io::Error| ClientError::Spawn(format!("{what}: {e}"))
        };
        paths::prepare_state(&self.state_dir).map_err(spawn_err("state directory"))?;
        let log_path = paths::log_file(&self.state_dir);
        if fs::metadata(&log_path).is_ok_and(|m| m.len() > LOG_ROTATE_BYTES) {
            let mut old = log_path.clone().into_os_string();
            old.push(".1");
            fs::rename(&log_path, old).ok();
        }
        let log = File::options()
            .create(true)
            .append(true)
            .open(&log_path)
            .map_err(spawn_err("host.log"))?;
        let mut cmd = Command::new(exe);
        cmd.arg("host")
            .env_clear()
            .envs(crate::env::scrubbed())
            .env("ARTALDE_STATE_DIR", &self.state_dir)
            .current_dir("/")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(log);
        crate::sys::new_session(&mut cmd, false);
        cmd.spawn().map_err(spawn_err(&exe.display().to_string()))
    }

    fn link(&self) -> Result<Arc<Link>, ClientError> {
        self.conn
            .locked()
            .link
            .as_ref()
            .filter(|l| l.is_connected())
            .map(Arc::clone)
            .ok_or(ClientError::Disconnected)
    }

    fn call_on<T: DeserializeOwned>(
        &self,
        link: &Link,
        method: &str,
        params: &Value,
    ) -> Result<T, ClientError> {
        let (tx, rx) = mpsc::sync_channel(1);
        let id = link.request(method, params, Pending::Call(tx))?;
        let value = match rx.recv_timeout(self.timeout) {
            Ok(outcome) => outcome?,
            Err(RecvTimeoutError::Timeout) => {
                link.state.locked().pending.remove(&id);
                return Err(ClientError::Timeout);
            }
            Err(RecvTimeoutError::Disconnected) => return Err(ClientError::Disconnected),
        };
        serde_json::from_value(value).map_err(|e| ClientError::Protocol(format!("{method}: {e}")))
    }

    fn call<T: DeserializeOwned>(&self, method: &str, params: &Value) -> Result<T, ClientError> {
        let link = self.link()?;
        self.call_on(&link, method, params)
    }

    /// The host on the other end, as it answered when the connection was
    /// made.
    pub fn host(&self) -> Option<HostInfo> {
        self.conn.locked().info.clone()
    }

    /// Whether the connection is up. Once it is not, every call fails with
    /// [`ClientError::Disconnected`] until [`Client::reconnect`].
    pub fn is_connected(&self) -> bool {
        self.link().is_ok()
    }

    /// Sets the function called, from the reader thread, when something
    /// arrived: output, an event, an end, a disconnect. It is called once,
    /// and not again until [`Client::woken`] has been called; so call
    /// `woken` first and then take everything there is.
    pub fn set_waker(&self, waker: Arc<dyn Fn() + Send + Sync>) {
        *self.wake.waker.locked() = Some(waker);
    }

    pub fn clear_waker(&self) {
        *self.wake.waker.locked() = None;
    }

    /// Whether something arrived since the last call. Re-arms the waker.
    pub fn woken(&self) -> bool {
        self.wake.woken.swap(false, Ordering::AcqRel)
    }

    /// Starts a terminal.
    pub fn run(&self, options: &RunOptions) -> Result<TerminalInfo, ClientError> {
        let params = serde_json::to_value(options).map_err(io::Error::other)?;
        self.call("run", &params)
    }

    /// Every terminal the host knows, running or finished, by id.
    pub fn list(&self) -> Result<Vec<TerminalInfo>, ClientError> {
        self.call("list", &Value::Null)
    }

    pub fn get(&self, terminal: u32) -> Result<TerminalInfo, ClientError> {
        self.call("get", &json!({ "terminal": terminal }))
    }

    /// The last `lines` lines of the terminal's output as plain text.
    pub fn read(&self, terminal: u32, lines: usize) -> Result<String, ClientError> {
        #[derive(Deserialize)]
        struct Text {
            text: String,
        }
        let text: Text = self.call("read", &json!({ "terminal": terminal, "lines": lines }))?;
        Ok(text.text)
    }

    /// Pastes `text` into the terminal and, with `submit`, presses Enter a
    /// moment later. Returns once it is queued, not once it is typed.
    pub fn send(&self, terminal: u32, text: &str, submit: bool) -> Result<(), ClientError> {
        self.call(
            "send",
            &json!({ "terminal": terminal, "text": text, "submit": submit }),
        )
    }

    /// Ends the terminal's program and returns once it is gone, which takes
    /// up to a few seconds for one that ignores signals.
    pub fn kill(&self, terminal: u32) -> Result<TerminalInfo, ClientError> {
        self.call("kill", &json!({ "terminal": terminal }))
    }

    /// Forgets a finished terminal; with `force`, ends a running one first.
    pub fn remove(&self, terminal: u32, force: bool) -> Result<(), ClientError> {
        self.call("remove", &json!({ "terminal": terminal, "force": force }))
    }

    pub fn rename(&self, terminal: u32, name: &str) -> Result<TerminalInfo, ClientError> {
        self.call("rename", &json!({ "terminal": terminal, "name": name }))
    }

    /// Sets a label, or with `None` removes it.
    pub fn label(
        &self,
        terminal: u32,
        key: &str,
        value: Option<&str>,
    ) -> Result<TerminalInfo, ClientError> {
        self.call(
            "label",
            &json!({ "terminal": terminal, "key": key, "value": value }),
        )
    }

    /// Stops the host and returns once it is gone. Refused while terminals
    /// run, unless `force`, which ends them.
    pub fn shutdown(&self, force: bool) -> Result<(), ClientError> {
        let link = self.link()?;
        self.call_on::<Value>(&link, "shutdown", &json!({ "force": force }))?;
        let deadline = Instant::now() + self.timeout;
        while link.is_connected() || !self.lock_is_free() {
            if Instant::now() >= deadline {
                return Err(ClientError::Timeout);
            }
            thread::sleep(Duration::from_millis(5));
        }
        Self::hang_up(&mut self.conn.locked());
        Ok(())
    }

    /// Opens a view of the terminal `cols` by `rows`: the stream starts
    /// with a replay of the screen (and, with `history`, of the output kept
    /// from before it) and goes on with what the program writes. The
    /// terminal takes this view's size. A second attach to the same
    /// terminal on this client ends the first.
    pub fn attach(
        &self,
        terminal: u32,
        cols: u16,
        rows: u16,
        history: bool,
    ) -> Result<Stream, ClientError> {
        let link = self.link()?;
        let shared = Arc::new(StreamShared::default());
        {
            let mut buf = shared.buf.locked();
            buf.cols = cols;
            buf.rows = rows;
            buf.history = history;
        }
        // An earlier stream of this terminal ends here, before the request
        // goes out, so that its detach cannot follow the new attach.
        if let Some(old) = link.state.locked().streams.remove(&terminal) {
            old.end(Exit::Failed("attached again".to_owned()));
        }
        let (tx, rx) = mpsc::sync_channel(1);
        let id = link.request(
            "attach",
            &json!({ "terminal": terminal, "cols": cols, "rows": rows, "history": history }),
            Pending::Attach {
                terminal,
                stream: Arc::clone(&shared),
                reply: Some(tx),
            },
        )?;
        let value = match rx.recv_timeout(self.timeout) {
            Ok(outcome) => outcome?,
            Err(RecvTimeoutError::Timeout) => {
                // The reply may still come and make the host stream to
                // nobody; the detach is queued behind the attach.
                link.state.locked().pending.remove(&id);
                link.send(Frame::new(Kind::Detach, terminal, Vec::new()))
                    .ok();
                return Err(ClientError::Timeout);
            }
            Err(RecvTimeoutError::Disconnected) => return Err(ClientError::Disconnected),
        };
        let info = serde_json::from_value(value)
            .map_err(|e| ClientError::Protocol(format!("attach: {e}")))?;
        Ok(Stream {
            link,
            terminal,
            shared,
            info,
        })
    }

    /// Starts the feed of changes. One feed per connection: a second call
    /// takes over from the first. See the crate documentation for the order
    /// that keeps a model exact.
    pub fn subscribe(&self) -> Result<Events, ClientError> {
        let link = self.link()?;
        let shared = Arc::new(EventsShared::default());
        link.state.locked().events = Some(Arc::clone(&shared));
        self.call_on::<Value>(&link, "subscribe", &Value::Null)?;
        Ok(Events { link, shared })
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        Self::hang_up(&mut self.conn.locked());
    }
}

/// One attached terminal. Dropping it detaches.
#[derive(Debug)]
pub struct Stream {
    link: Arc<Link>,
    terminal: u32,
    shared: Arc<StreamShared>,
    info: TerminalInfo,
}

impl Stream {
    pub fn terminal(&self) -> u32 {
        self.terminal
    }

    /// The terminal as it was when attached.
    pub fn info(&self) -> &TerminalInfo {
        &self.info
    }

    /// Appends what has arrived to `out`; `true` when that was anything.
    /// Feed it to an emulator as is: when the stream had to skip output it
    /// could not hold, what comes next starts with a reset and a replay.
    pub fn try_read(&mut self, out: &mut Vec<u8>) -> bool {
        let mut buf = self.shared.buf.locked();
        if buf.lagged && !buf.reattaching && buf.ended.is_none() {
            buf.reattaching = true;
            let params = json!({
                "terminal": self.terminal,
                "cols": buf.cols,
                "rows": buf.rows,
                "history": buf.history,
            });
            drop(buf);
            let pending = Pending::Attach {
                terminal: self.terminal,
                stream: Arc::clone(&self.shared),
                reply: None,
            };
            if self.link.request("attach", &params, pending).is_err() {
                self.shared.buf.locked().reattaching = false;
            }
            return false;
        }
        let any = buf.bytes > 0;
        for chunk in buf.chunks.drain(..) {
            out.extend_from_slice(&chunk);
        }
        buf.bytes = 0;
        any
    }

    /// One piece of what has arrived, as the host framed it. Unlike
    /// [`Stream::try_read`] this keeps a replay apart from what follows it:
    /// a replay is always a piece of its own and starts with `CAN ESC c`.
    pub fn try_next(&mut self) -> Option<Vec<u8>> {
        {
            let mut buf = self.shared.buf.locked();
            if !(buf.lagged && !buf.reattaching && buf.ended.is_none()) {
                let chunk = buf.chunks.pop_front()?;
                buf.bytes = buf.bytes.saturating_sub(chunk.len());
                return Some(chunk);
            }
        }
        // Fell behind: `try_read` asks for the replay.
        self.try_read(&mut Vec::new());
        None
    }

    /// How the stream ended, once everything before the end has been read.
    pub fn ended(&self) -> Option<Exit> {
        let buf = self.shared.buf.locked();
        if buf.bytes > 0 {
            return None;
        }
        buf.ended.clone()
    }

    /// Sends input to the program.
    pub fn write(&self, bytes: &[u8]) -> Result<(), ClientError> {
        for chunk in bytes.chunks(INPUT_CHUNK) {
            self.link
                .send(Frame::new(Kind::Input, self.terminal, chunk.to_vec()))?;
        }
        Ok(())
    }

    /// Tells the host the size of this view, in cells and pixels per cell;
    /// the terminal takes it. Sending the unchanged size is how a view that
    /// got the focus claims the terminal's size back.
    pub fn resize(
        &self,
        cols: u16,
        rows: u16,
        cell_w: u16,
        cell_h: u16,
    ) -> Result<(), ClientError> {
        {
            let mut buf = self.shared.buf.locked();
            buf.cols = cols;
            buf.rows = rows;
        }
        self.link.send(Frame::new(
            Kind::Resize,
            self.terminal,
            encode_resize(cols, rows, cell_w, cell_h),
        ))
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        let mut state = self.link.state.locked();
        let ours = state
            .streams
            .get(&self.terminal)
            .is_some_and(|s| Arc::ptr_eq(s, &self.shared));
        if ours {
            state.streams.remove(&self.terminal);
            drop(state);
            self.link
                .queue
                .push(Frame::new(Kind::Detach, self.terminal, Vec::new()));
        }
    }
}

/// The feed of changes to the host's terminals.
#[derive(Debug)]
pub struct Events {
    link: Arc<Link>,
    shared: Arc<EventsShared>,
}

impl Events {
    pub fn try_next(&mut self) -> Option<Event> {
        self.shared.queue.locked().pop_front()
    }

    /// Whether events were dropped because nobody took them, since the last
    /// call. The model is then behind: [`Client::list`] again.
    pub fn lagged(&mut self) -> bool {
        self.shared.lagged.swap(false, Ordering::AcqRel)
    }

    /// `false` once the connection is gone and no more events will come.
    pub fn is_connected(&self) -> bool {
        self.link.is_connected()
    }
}

impl Drop for Events {
    fn drop(&mut self) {
        let mut state = self.link.state.locked();
        if state
            .events
            .as_ref()
            .is_some_and(|e| Arc::ptr_eq(e, &self.shared))
        {
            state.events = None;
        }
    }
}
