pub mod api;
pub mod status;
pub mod store;

use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant};

pub use api::DEFAULT_PORT;

pub fn port() -> u16 {
    std::env::var("IKUSI_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(DEFAULT_PORT)
}

pub fn base(port: u16) -> String {
    format!("http://127.0.0.1:{port}")
}

/// The server's own `/__health`, asked the short way: sutegi is a server, not
/// a client, and a 40-line raw GET beats pulling an HTTP client into a crate
/// that has none.
pub fn is_up(port: u16, timeout: Duration) -> bool {
    let Some(addr) = format!("127.0.0.1:{port}").to_socket_addrs().ok().and_then(|mut a| a.next())
    else {
        return false;
    };
    let Ok(mut stream) = TcpStream::connect_timeout(&addr, timeout) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(timeout));
    let _ = stream.set_write_timeout(Some(timeout));
    if stream.write_all(b"GET /__health HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n").is_err() {
        return false;
    }
    let mut head = [0u8; 16];
    let mut seen = 0usize;
    while seen < head.len() {
        match stream.read(&mut head[seen..]) {
            Ok(0) => break,
            Ok(n) => seen += n,
            Err(_) => break,
        }
    }
    String::from_utf8_lossy(&head[..seen]).starts_with("HTTP/1.1 200")
}

/// Start the server in the background and wait for it to answer. Idempotent:
/// if something already answers on the port, nothing is spawned.
pub fn ensure_up(port: u16, root: Option<&str>) -> Result<bool, String> {
    if is_up(port, Duration::from_millis(400)) {
        return Ok(false);
    }
    let exe = std::env::current_exe().map_err(|e| format!("cannot find myself: {e}"))?;
    let log = log_path(root);
    if let Some(dir) = log.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let sink = || {
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log)
            .map(std::process::Stdio::from)
            .unwrap_or_else(|_| std::process::Stdio::null())
    };
    let spawn = || -> Result<std::process::Child, String> {
        let mut cmd = std::process::Command::new(&exe);
        cmd.arg("serve").arg("--port").arg(port.to_string());
        if let Some(r) = root {
            cmd.arg("--root").arg(r);
        }
        cmd.stdin(std::process::Stdio::null())
            .stdout(sink())
            .stderr(sink())
            .spawn()
            .map_err(|e| format!("cannot start ikusi: {e}"))
    };

    // Retried, because the one way this fails in practice is racing a server
    // that is still draining its own SIGTERM: sutegi joins its workers, and a
    // worker parked on a browser's keep-alive socket holds the port for a
    // second or two after the process was told to go. A cold start takes
    // ~0.2s, so the retry costs nothing when there is nothing to wait for.
    let give_up = Instant::now() + Duration::from_secs(15);
    loop {
        let mut child = spawn()?;
        let deadline = Instant::now() + Duration::from_secs(8);
        while Instant::now() < deadline {
            if is_up(port, Duration::from_millis(300)) {
                return Ok(true);
            }
            // A dead child means waiting out the deadline only delays the
            // error, and the reason for it is already in the log.
            if matches!(child.try_wait(), Ok(Some(_))) {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        let _ = child.kill();
        let _ = child.wait();
        if is_up(port, Duration::from_millis(400)) {
            return Ok(false);
        }
        if Instant::now() >= give_up {
            break;
        }
        std::thread::sleep(Duration::from_millis(400));
    }
    Err(format!(
        "ikusi did not come up on port {port} — last lines of {}:\n{}",
        log.display(),
        tail(&log, 3)
    ))
}

fn tail(path: &std::path::Path, lines: usize) -> String {
    std::fs::read_to_string(path)
        .map(|t| {
            t.lines()
                .rev()
                .take(lines)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .map(|l| format!("  {l}"))
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default()
}

fn log_path(root: Option<&str>) -> std::path::PathBuf {
    crate::kg::Paths::resolve(root).root.join("scripts/ikusi.log")
}

/// A deep link to one node. Ids carry `/`, `#` and `:`, so the fragment is
/// percent-encoded — an unescaped `#` in `gh:join-com/data#7280` would cut the
/// URL in half.
pub fn node_url(port: u16, id: Option<&str>) -> String {
    match id {
        Some(id) => format!("{}/#/n/{}", base(port), encode(id)),
        None => format!("{}/", base(port)),
    }
}

pub fn encode(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len() + 8);
    for b in raw.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

/// The platform's "open this with the default handler" command.
pub fn opener() -> &'static str {
    if cfg!(target_os = "macos") { "open" } else { "xdg-open" }
}
