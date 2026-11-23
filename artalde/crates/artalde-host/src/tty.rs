//! Attaching the caller's own terminal to one of the host's: `artalde term
//! attach`.

use std::io::{self, PipeWriter, Write as _};
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::{Arc, Mutex, Once};

use rustix::event::{PollFd, PollFlags};
use rustix::fs::OFlags;
use rustix::io::Errno;
use rustix::termios::{OptionalActions, Termios};

use crate::client::{Client, ClientError, Exit};
use crate::util::LockExt as _;

/// `Ctrl+\`: leaves the terminal running and returns.
const DETACH_KEY: u8 = 0x1c;

/// Undoes what the attached program may have left the caller's terminal
/// in: alternate screen, mouse reporting, bracketed paste, a hidden cursor,
/// colours.
const TIDY: &[u8] =
    b"\x1b[?1049l\x1b[?1000l\x1b[?1002l\x1b[?1003l\x1b[?1006l\x1b[?2004l\x1b[?25h\x1b[0m\r\n";

/// How [`attach_tty`] ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttachEnd {
    /// The detach key; the program runs on.
    Detached,
    /// The program is gone.
    Exited {
        code: Option<i32>,
        signal: Option<i32>,
    },
    /// The host went away.
    Disconnected,
    /// This process was told to stop by the given signal.
    Signalled(i32),
}

/// The terminal settings to put back, wherever the process leaves from.
static SAVED: Mutex<Option<Termios>> = Mutex::new(None);

fn restore() {
    if let Some(saved) = SAVED.locked().take() {
        rustix::termios::tcsetattr(rustix::stdio::stdin(), OptionalActions::Now, &saved).ok();
    }
}

struct RawMode;

impl RawMode {
    fn enter() -> io::Result<RawMode> {
        static HOOK: Once = Once::new();
        let stdin = rustix::stdio::stdin();
        let saved = rustix::termios::tcgetattr(stdin)?;
        let mut raw = saved.clone();
        raw.make_raw();
        *SAVED.locked() = Some(saved);
        // A panic prints its message; put the terminal back first so it is
        // readable.
        HOOK.call_once(|| {
            let previous = std::panic::take_hook();
            std::panic::set_hook(Box::new(move |info| {
                restore();
                previous(info);
            }));
        });
        if let Err(e) = rustix::termios::tcsetattr(stdin, OptionalActions::Now, &raw) {
            restore();
            return Err(e.into());
        }
        Ok(RawMode)
    }
}

impl Drop for RawMode {
    fn drop(&mut self) {
        restore();
    }
}

fn window() -> (u16, u16, u16, u16) {
    let size = rustix::termios::tcgetwinsize(rustix::stdio::stdout())
        .or_else(|_| rustix::termios::tcgetwinsize(rustix::stdio::stdin()));
    match size {
        Ok(ws) if ws.ws_col > 0 && ws.ws_row > 0 => (
            ws.ws_col,
            ws.ws_row,
            ws.ws_xpixel / ws.ws_col,
            ws.ws_ypixel / ws.ws_row,
        ),
        _ => (80, 24, 0, 0),
    }
}

fn poke(pipe: &PipeWriter) {
    let mut pipe = pipe;
    // Full means a wake-up is pending already.
    pipe.write_all(&[0]).ok();
}

/// Shows terminal `terminal` on this process's terminal and passes the keys
/// through, until the program ends, the host goes, or `Ctrl+\` is typed.
/// The caller's terminal is in raw mode meanwhile and is put back on every
/// way out, a panic or a terminating signal included.
///
/// Takes over the client's waker.
pub fn attach_tty(client: &Client, terminal: u32, history: bool) -> Result<AttachEnd, ClientError> {
    use signal_hook::consts::{SIGHUP, SIGINT, SIGTERM, SIGWINCH};

    let stdin = rustix::stdio::stdin();
    if !rustix::termios::isatty(stdin) {
        return Err(ClientError::Io(io::Error::other(
            "standard input is not a terminal",
        )));
    }
    let (wake_r, wake_w) = io::pipe()?;
    let flags = rustix::fs::fcntl_getfl(&wake_w).map_err(io::Error::from)?;
    rustix::fs::fcntl_setfl(&wake_w, flags | OFlags::NONBLOCK).map_err(io::Error::from)?;
    let wake_w = Arc::new(wake_w);

    let resized = Arc::new(AtomicBool::new(false));
    let stopped = Arc::new(AtomicI32::new(0));
    let mut signals = signal_hook::iterator::Signals::new([SIGWINCH, SIGTERM, SIGHUP, SIGINT])?;
    let closer = signals.handle();
    let (for_resized, for_stopped, for_poke) = (
        Arc::clone(&resized),
        Arc::clone(&stopped),
        Arc::clone(&wake_w),
    );
    let watcher = std::thread::Builder::new()
        .name("artalde attach signals".into())
        .spawn(move || {
            for signal in signals.forever() {
                if signal == SIGWINCH {
                    for_resized.store(true, Ordering::Release);
                } else {
                    for_stopped.store(signal, Ordering::Release);
                }
                poke(&for_poke);
            }
        })?;

    let for_waker = Arc::clone(&wake_w);
    client.set_waker(Arc::new(move || poke(&for_waker)));
    let outcome = pass_through(client, terminal, history, &wake_r, &resized, &stopped);
    client.clear_waker();
    closer.close();
    watcher.join().ok();
    outcome
}

fn pass_through(
    client: &Client,
    terminal: u32,
    history: bool,
    wake: &io::PipeReader,
    resized: &AtomicBool,
    stopped: &AtomicI32,
) -> Result<AttachEnd, ClientError> {
    let stdin = rustix::stdio::stdin();
    let (cols, rows, _, _) = window();
    let mut stream = client.attach(terminal, cols, rows, history)?;
    let raw = RawMode::enter()?;
    let mut stdout = io::stdout().lock();
    let mut output = Vec::new();
    let mut keys = [0u8; 4096];
    let mut scratch = [0u8; 256];
    let end = loop {
        client.woken();
        if resized.swap(false, Ordering::AcqRel) {
            let (cols, rows, cell_w, cell_h) = window();
            stream.resize(cols, rows, cell_w, cell_h)?;
        }
        output.clear();
        while stream.try_read(&mut output) {}
        if !output.is_empty() {
            stdout.write_all(&output)?;
            stdout.flush()?;
        }
        match stream.ended() {
            Some(Exit::Exited { code, signal }) => break AttachEnd::Exited { code, signal },
            Some(Exit::Disconnected) => break AttachEnd::Disconnected,
            Some(Exit::Failed(why)) => {
                drop(raw);
                return Err(ClientError::Host(why));
            }
            None => {}
        }
        let signal = stopped.load(Ordering::Acquire);
        if signal != 0 {
            break AttachEnd::Signalled(signal);
        }

        let mut fds = [
            PollFd::new(&stdin, PollFlags::IN),
            PollFd::new(wake, PollFlags::IN),
        ];
        match rustix::event::poll(&mut fds, None) {
            Ok(_) => {}
            Err(Errno::INTR) => continue,
            Err(e) => return Err(io::Error::from(e).into()),
        }
        let (keys_ready, wake_ready) = (!fds[0].revents().is_empty(), !fds[1].revents().is_empty());
        if wake_ready {
            // One read: the pipe holds at most a handful of bytes, and what
            // is left only costs another turn of the loop.
            rustix::io::read(wake, &mut scratch).ok();
        }
        if keys_ready {
            let n = match rustix::io::read(stdin, &mut keys) {
                Ok(0) => break AttachEnd::Detached,
                Ok(n) => n,
                Err(Errno::INTR | Errno::AGAIN) => continue,
                Err(e) => return Err(io::Error::from(e).into()),
            };
            let typed = &keys[..n];
            match typed.iter().position(|&b| b == DETACH_KEY) {
                Some(at) => {
                    if at > 0 {
                        stream.write(&typed[..at])?;
                    }
                    break AttachEnd::Detached;
                }
                None => stream.write(typed)?,
            }
        }
    };
    drop(stream);
    stdout.write_all(TIDY).ok();
    stdout.flush().ok();
    drop(raw);
    Ok(end)
}
