//! The host and its client against real PTYs and real programs. Every test
//! has its own state directory and a host on threads of the test process;
//! stopping that host ends every program it started.

#![allow(
    clippy::unwrap_used,
    reason = "a test that cannot go on should say where it stopped"
)]

use std::io::{Read as _, Write as _};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use artalde_host::wire::{self, Frame, Kind};
use artalde_host::{
    Client, ClientError, Event, Events, Exit, Host, HostError, HostHandle, RunOptions, Spawn,
    State, StatusState, Stream,
};

const PATIENCE: Duration = Duration::from_secs(20);

struct Fixture {
    dir: PathBuf,
    host: Option<HostHandle>,
}

impl Fixture {
    fn new(name: &str) -> Fixture {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "artalde-host-{}-{}-{name}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::remove_dir_all(&dir).ok();
        let host = Host::serve(&dir).unwrap();
        Fixture {
            dir,
            host: Some(host),
        }
    }

    fn client(&self) -> Client {
        Client::connect(&self.dir, Spawn::Never).unwrap()
    }

    fn host(&self) -> &HostHandle {
        self.host.as_ref().unwrap()
    }

    fn stop(&mut self) {
        if let Some(host) = self.host.take() {
            host.stop();
        }
    }

    fn start(&mut self) {
        self.host = Some(Host::serve(&self.dir).unwrap());
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.stop();
        std::fs::remove_dir_all(&self.dir).ok();
    }
}

fn wait_for<T>(what: &str, mut check: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + PATIENCE;
    loop {
        if let Some(value) = check() {
            return value;
        }
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn sh(script: &str) -> RunOptions {
    RunOptions {
        argv: vec!["/bin/sh".into(), "-c".into(), script.into()],
        cwd: Some("/".into()),
        ..RunOptions::default()
    }
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

/// Reads until `seen` holds `needle`.
fn read_until(stream: &mut Stream, seen: &mut Vec<u8>, needle: &str) {
    wait_for(needle, || {
        stream.try_read(seen);
        if contains(seen, needle.as_bytes()) {
            return Some(());
        }
        assert_eq!(
            stream.ended(),
            None,
            "ended before {needle:?}: {:?}",
            String::from_utf8_lossy(seen)
        );
        None
    });
}

/// Reads to the end of the stream.
fn read_to_end(stream: &mut Stream, seen: &mut Vec<u8>) -> Exit {
    wait_for("the end of the stream", || {
        stream.try_read(seen);
        stream.ended()
    })
}

fn next_event(events: &mut Events, what: &str, mut want: impl FnMut(&Event) -> bool) -> Event {
    wait_for(what, || {
        while let Some(event) = events.try_next() {
            if want(&event) {
                return Some(event);
            }
        }
        None
    })
}

fn alive(pid: u32) -> bool {
    Command::new("/bin/kill")
        .args(["-0", &pid.to_string()])
        .stderr(std::process::Stdio::null())
        .status()
        .unwrap()
        .success()
}

fn wait_dead(pid: u32) {
    wait_for("the process to be gone", || (!alive(pid)).then_some(()));
}

fn exited(client: &Client, id: u32) -> (Option<i32>, Option<i32>) {
    wait_for("the terminal to finish", || {
        match client.get(id).unwrap().state {
            State::Exited { code, signal, .. } => Some((code, signal)),
            State::Running => None,
        }
    })
}

#[test]
fn hello_then_cat_echoes_what_is_typed() {
    let fx = Fixture::new("cat");
    let client = fx.client();
    let host = client.host().unwrap();
    assert_eq!(host.pid, std::process::id());
    assert_eq!(host.version, wire::PROTOCOL_VERSION);

    let info = client
        .run(&RunOptions {
            argv: vec!["/bin/cat".into()],
            name: Some("kitty".into()),
            ..RunOptions::default()
        })
        .unwrap();
    assert_eq!(info.name, "kitty");
    assert!(info.state.is_running());
    let mut stream = client.attach(info.id, 80, 24, false).unwrap();
    assert_eq!(stream.info().id, info.id);
    stream.write(b"hello\n").unwrap();
    let mut seen = Vec::new();
    // Once echoed by the terminal, once written by cat.
    read_until(&mut stream, &mut seen, "hello\r\nhello\r\n");
    assert!(seen.starts_with(b"\x18\x1bc"), "replay comes first");
    assert_eq!(client.get(info.id).unwrap().viewers, 1);
    assert_eq!(client.read(info.id, 10).unwrap(), "hello\nhello");
    drop(stream);
    wait_for("the detach", || {
        (client.get(info.id).unwrap().viewers == 0).then_some(())
    });
}

#[test]
fn environment_and_controlling_terminal() {
    let fx = Fixture::new("env");
    let client = fx.client();
    let info = client
        .run(&RunOptions {
            env: [("EXTRA".to_owned(), "yes".to_owned())].into(),
            cwd: Some("/usr".into()),
            ..sh(
                "echo \"T=$ARTALDE_TERMINAL TERM=$TERM P=$TERM_PROGRAM E=$EXTRA \
                 C=${CLAUDECODE:-unset} S=$ARTALDE_HOST_SOCKET\"; pwd; tty; echo END; sleep 600",
            )
        })
        .unwrap();
    let mut stream = client.attach(info.id, 80, 24, true).unwrap();
    let mut seen = Vec::new();
    read_until(&mut stream, &mut seen, "END");
    let text = client.read(info.id, 10).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(
        lines[0],
        format!(
            "T={} TERM=xterm-256color P=artalde E=yes C=unset S={}",
            info.id,
            fx.host().socket().display()
        )
    );
    assert_eq!(lines[1], "/usr");
    assert!(lines[2].starts_with("/dev/"), "{}", lines[2]);
    assert_eq!(info.cwd, "/usr");
    assert_eq!(info.name, "sh");
}

#[test]
fn the_terminal_takes_the_viewers_size() {
    let fx = Fixture::new("size");
    let client = fx.client();
    let mut events = client.subscribe().unwrap();
    let info = client
        .run(&RunOptions {
            argv: vec!["/bin/sh".into()],
            ..RunOptions::default()
        })
        .unwrap();
    assert_eq!((info.cols, info.rows), (80, 24));
    let mut stream = client.attach(info.id, 90, 33, false).unwrap();
    assert_eq!((stream.info().cols, stream.info().rows), (90, 33));
    let mut seen = Vec::new();
    stream.write(b"stty size\n").unwrap();
    read_until(&mut stream, &mut seen, "33 90\r\n");
    stream.resize(101, 37, 8, 16).unwrap();
    next_event(&mut events, "the size event", |e| {
        matches!(
            e,
            Event::Size {
                cols: 101,
                rows: 37,
                ..
            }
        )
    });
    stream.write(b"stty size\n").unwrap();
    read_until(&mut stream, &mut seen, "37 101\r\n");

    // A second viewer takes the size over by attaching, the first takes it
    // back by typing.
    let other = fx.client();
    let _second = other.attach(info.id, 60, 20, false).unwrap();
    assert_eq!(client.get(info.id).unwrap().cols, 60);
    seen.clear();
    stream.write(b"stty size\n").unwrap();
    read_until(&mut stream, &mut seen, "37 101\r\n");
    assert_eq!(client.get(info.id).unwrap().cols, 101);
}

/// The lines of a stream that was attached with history while the program
/// counted: after the reset, every number from zero, each once.
fn counted(seen: &[u8]) -> usize {
    let body = seen
        .strip_prefix(b"\x18\x1bc")
        .expect("starts with a reset");
    assert!(!contains(body, b"\x18\x1bc"), "one replay only");
    let text = std::str::from_utf8(body).unwrap();
    let mut next = 0;
    for line in text.split_inclusive("\r\n") {
        let Some(line) = line.strip_suffix("\r\n") else {
            // The last line is still on its way.
            break;
        };
        if line == "END" {
            break;
        }
        assert_eq!(line, format!("n{next}"), "after line {next}");
        next += 1;
    }
    next
}

#[test]
fn attaching_late_misses_nothing_and_doubles_nothing() {
    const LINES: usize = 30_000;
    let fx = Fixture::new("replay");
    let client = fx.client();
    let info = client
        .run(&sh(&format!(
            "i=0; while [ $i -lt {LINES} ]; do echo n$i; i=$((i+1)); done; echo END; sleep 600"
        )))
        .unwrap();
    let mut attaches = 0;
    let mut progress = 0;
    loop {
        // Each attach replays from the first line and joins the live output
        // somewhere in the middle of the count.
        let mut stream = client.attach(info.id, 80, 24, true).unwrap();
        attaches += 1;
        let mut seen = Vec::new();
        let done = wait_for("more lines", || {
            stream.try_read(&mut seen);
            if contains(&seen, b"END\r\n") {
                return Some(true);
            }
            let lines = seen.iter().filter(|&&b| b == b'\n').count();
            (lines > progress + 500).then_some(false)
        });
        let lines = counted(&seen);
        assert!(lines >= progress, "{lines} lines after {progress}");
        progress = lines;
        if done {
            assert_eq!(lines, LINES);
            break;
        }
    }
    assert!(attaches > 1, "the program finished before a second attach");
}

#[test]
fn two_viewers_see_the_same_output() {
    let fx = Fixture::new("two");
    let (a, b) = (fx.client(), fx.client());
    let info = a.run(&sh("stty -echo; echo ready; exec cat")).unwrap();
    let mut first = a.attach(info.id, 80, 24, true).unwrap();
    let mut seen_first = Vec::new();
    read_until(&mut first, &mut seen_first, "ready\r\n");
    let mut second = b.attach(info.id, 80, 24, true).unwrap();
    assert_eq!(a.get(info.id).unwrap().viewers, 2);
    first.write(b"from-a\n").unwrap();
    second.write(b"from-b\n").unwrap();
    let mut seen_second = Vec::new();
    for (stream, seen) in [
        (&mut first, &mut seen_first),
        (&mut second, &mut seen_second),
    ] {
        read_until(stream, seen, "from-a\r\n");
        read_until(stream, seen, "from-b\r\n");
    }
    assert_eq!(seen_first, seen_second);
}

#[test]
fn exit_is_reported_with_its_code_after_the_last_output() {
    let fx = Fixture::new("exit");
    let client = fx.client();
    let mut events = client.subscribe().unwrap();
    let info = client
        .run(&sh(
            "i=0; while [ $i -lt 3000 ]; do echo line$i; i=$((i+1)); done; echo LAST; exit 7",
        ))
        .unwrap();
    let mut stream = client.attach(info.id, 80, 24, true).unwrap();
    let mut seen = Vec::new();
    let exit = read_to_end(&mut stream, &mut seen);
    assert_eq!(
        exit,
        Exit::Exited {
            code: Some(7),
            signal: None
        }
    );
    assert!(seen.ends_with(b"line2999\r\nLAST\r\n"));
    let event = next_event(&mut events, "exited", |e| matches!(e, Event::Exited { .. }));
    assert!(matches!(
        event,
        Event::Exited {
            code: Some(7),
            signal: None,
            ..
        }
    ));
    let after = client.get(info.id).unwrap();
    assert!(matches!(
        after.state,
        State::Exited {
            code: Some(7),
            signal: None,
            lost: false,
            ..
        }
    ));
    assert_eq!(after.pid, None);
    wait_dead(info.pid.unwrap());

    // A finished terminal can still be read and attached to.
    assert!(client.read(info.id, 1).unwrap().ends_with("LAST"));
    let mut again = client.attach(info.id, 80, 24, true).unwrap();
    let mut replayed = Vec::new();
    assert_eq!(read_to_end(&mut again, &mut replayed), exit);
    assert_eq!(replayed, seen);
    assert!(matches!(
        again.write(b"x"),
        Ok(()) | Err(ClientError::Disconnected)
    ));
}

#[test]
fn death_by_signal_is_reported_as_one() {
    let fx = Fixture::new("signal");
    let client = fx.client();
    let info = client.run(&sh("echo dying; kill -9 $$")).unwrap();
    assert_eq!(exited(&client, info.id), (None, Some(9)));
    assert_eq!(client.read(info.id, 5).unwrap(), "dying");
}

#[test]
fn kill_escalates_until_the_program_is_gone() {
    let fx = Fixture::new("kill");
    let client = fx.client();

    let polite = client.run(&sh("echo ready; sleep 600")).unwrap();
    let stubborn = client
        .run(&sh(
            "trap '' HUP TERM; echo ready; while :; do sleep 600; done",
        ))
        .unwrap();
    for id in [polite.id, stubborn.id] {
        let mut stream = client.attach(id, 80, 24, true).unwrap();
        read_until(&mut stream, &mut Vec::new(), "ready");
    }

    let started = Instant::now();
    let after = client.kill(polite.id).unwrap();
    assert!(matches!(
        after.state,
        State::Exited {
            signal: Some(1),
            ..
        }
    ));
    assert!(started.elapsed() < Duration::from_millis(900));
    wait_dead(polite.pid.unwrap());

    let started = Instant::now();
    let after = client.kill(stubborn.id).unwrap();
    assert!(
        matches!(
            after.state,
            State::Exited {
                signal: Some(9),
                ..
            }
        ),
        "{:?}",
        after.state
    );
    let took = started.elapsed();
    assert!(
        took >= Duration::from_millis(900) && took < Duration::from_secs(4),
        "{took:?}"
    );
    wait_dead(stubborn.pid.unwrap());
    // Killing what is dead already is not an error.
    assert!(!client.kill(stubborn.id).unwrap().state.is_running());
}

#[test]
fn send_pastes_and_submits() {
    let fx = Fixture::new("send");
    let client = fx.client();

    let plain = client
        .run(&sh("stty raw -echo; echo ready; exec cat -v"))
        .unwrap();
    let mut stream = client.attach(plain.id, 80, 24, true).unwrap();
    let mut seen = Vec::new();
    read_until(&mut stream, &mut seen, "ready");
    seen.clear();
    client.send(plain.id, "one\ntwo", false).unwrap();
    let started = Instant::now();
    client.send(plain.id, "three", true).unwrap();
    read_until(&mut stream, &mut seen, "three^M");
    assert_eq!(seen, b"one^Mtwothree^M");
    assert!(
        started.elapsed() >= Duration::from_millis(150),
        "Enter follows the text after a pause"
    );

    let bracketed = client
        .run(&sh(
            "stty raw -echo; printf '\\033[?2004hready'; exec cat -v",
        ))
        .unwrap();
    let mut stream = client.attach(bracketed.id, 80, 24, true).unwrap();
    let mut seen = Vec::new();
    read_until(&mut stream, &mut seen, "ready");
    seen.clear();
    client.send(bracketed.id, "pasted\ntext", true).unwrap();
    read_until(&mut stream, &mut seen, "~^M");
    assert_eq!(seen, b"^[[200~pasted^Mtext^[[201~^M");

    client.kill(plain.id).unwrap();
    let refused = client.send(plain.id, "late", true);
    assert!(matches!(refused, Err(ClientError::Host(_))), "{refused:?}");
}

#[test]
fn events_follow_a_terminal_through_its_life() {
    let fx = Fixture::new("events");
    let client = fx.client();
    let mut events = client.subscribe().unwrap();
    let info = client
        .run(&RunOptions {
            name: Some("agent".into()),
            labels: [("role".to_owned(), "test".to_owned())].into(),
            ..sh(
                "printf '\\033]2;my title\\007\\033]7;file://host/tmp\\007\\007\
                 \\033]7501;state=working:app=tester:msg=aGk=\\033\\\\\
                 \\033]7501;state=done\\007\
                 \\033]7501;state=working:progress=50\\007\
                 \\033]7501;state=blocked:kind=permission:title=d2FpdA==\\007'; sleep 600",
            )
        })
        .unwrap();
    let id = info.id;

    let created = next_event(&mut events, "created", |_| true);
    let Event::Created { terminal, info } = created else {
        panic!("first event is {created:?}");
    };
    assert_eq!((terminal, info.name.as_str()), (id, "agent"));
    assert_eq!(info.labels["role"], "test");

    let mut statuses = Vec::new();
    let (mut title, mut cwd, mut bell) = (None, None, false);
    while statuses.len() < 4 {
        match next_event(&mut events, "status events", |e| e.terminal() == id) {
            Event::Title { title: t, .. } => title = t,
            Event::Cwd { cwd: c, .. } => cwd = Some(c),
            Event::Bell { .. } => bell = true,
            Event::Status { status, .. } => {
                let status = status.unwrap();
                statuses.push((status.state, status.turn));
            }
            Event::Foreground { .. } => {}
            other => panic!("unexpected {other:?}"),
        }
    }
    assert_eq!(title.as_deref(), Some("my title"));
    assert_eq!(cwd.as_deref(), Some("/tmp"));
    assert!(bell);
    assert_eq!(
        statuses,
        [
            (StatusState::Working, 0),
            (StatusState::Done, 1),
            (StatusState::Working, 1),
            (StatusState::Blocked, 2),
        ]
    );
    let now = client.get(id).unwrap();
    let status = now.status.unwrap();
    assert_eq!(status.state, StatusState::Blocked);
    assert_eq!(status.kind.as_deref(), Some("permission"));
    assert_eq!(status.title.as_deref(), Some("wait"));
    assert_eq!(status.turn, 2);
    assert_eq!(now.title.as_deref(), Some("my title"));
    assert_eq!(now.cwd, "/tmp");

    client.rename(id, "renamed").unwrap();
    client.label(id, "role", None).unwrap();
    client.label(id, "k", Some("v")).unwrap();
    next_event(
        &mut events,
        "renamed",
        |e| matches!(e, Event::Renamed { name, .. } if name == "renamed"),
    );
    next_event(
        &mut events,
        "labels",
        |e| matches!(e, Event::Labels { labels, .. } if labels.len() == 1 && labels["k"] == "v"),
    );

    client.kill(id).unwrap();
    // The status goes with the program that set it, and before the exit.
    next_event(&mut events, "status cleared", |e| {
        matches!(e, Event::Status { status: None, .. })
    });
    next_event(&mut events, "exited", |e| matches!(e, Event::Exited { .. }));
    assert!(matches!(
        client.remove(999, false),
        Err(ClientError::Host(_))
    ));
    client.remove(id, false).unwrap();
    next_event(&mut events, "removed", |e| {
        matches!(e, Event::Removed { .. })
    });
    assert!(client.list().unwrap().is_empty());
    assert!(!events.lagged());
}

#[test]
fn foreground_program_is_named() {
    let fx = Fixture::new("fg");
    let client = fx.client();
    let info = client
        .run(&RunOptions {
            argv: vec!["/bin/sh".into()],
            ..RunOptions::default()
        })
        .unwrap();
    let foreground = |want: &dyn Fn(&str) -> bool| {
        wait_for("the foreground name", || {
            let name = client.get(info.id).unwrap().foreground?;
            want(&name).then_some(name)
        })
    };
    // What `/bin/sh` is differs: bash on macOS, dash on Debian.
    let shell = foreground(&|name| name != "sleep");
    assert!(shell.ends_with("sh"), "{shell}");
    // An interactive shell gives the terminal to the job it starts.
    let stream = client.attach(info.id, 80, 24, false).unwrap();
    stream.write(b"sleep 600\n").unwrap();
    foreground(&|name| name == "sleep");
}

#[test]
fn the_registry_survives_the_host() {
    let mut fx = Fixture::new("persist");
    let client = fx.client();
    let running = client.run(&sh("sleep 600")).unwrap();
    let finished = client.run(&sh("exit 3")).unwrap();
    assert_eq!(exited(&client, finished.id), (Some(3), None));
    client.label(running.id, "keep", Some("me")).unwrap();
    drop(client);

    // A host that dies takes no leave: what it last wrote says "running".
    fx.host.take().unwrap().abandon();
    wait_dead(running.pid.unwrap());
    fx.start();
    let client = fx.client();
    let list = client.list().unwrap();
    assert_eq!(list.len(), 2);
    assert_eq!(list[0].id, running.id);
    assert!(matches!(
        list[0].state,
        State::Exited {
            code: None,
            signal: None,
            lost: true,
            ..
        }
    ));
    assert_eq!(list[0].labels["keep"], "me");
    assert_eq!(list[0].pid, None);
    assert!(matches!(
        list[1].state,
        State::Exited {
            code: Some(3),
            lost: false,
            ..
        }
    ));
    // Ids are not handed out twice, not even those of removed terminals.
    client.remove(finished.id, false).unwrap();
    client.remove(running.id, false).unwrap();
    let next = client.run(&sh("sleep 600")).unwrap();
    assert!(next.id > finished.id);
    drop(client);

    // A host that stops in order records how its terminals ended.
    fx.stop();
    fx.start();
    let client = fx.client();
    let list = client.list().unwrap();
    assert_eq!(list.len(), 1);
    assert!(matches!(
        list[0].state,
        State::Exited {
            signal: Some(1),
            lost: false,
            ..
        }
    ));
    let newer = client.run(&sh("sleep 600")).unwrap();
    assert!(newer.id > next.id);
}

#[test]
fn a_second_host_is_refused() {
    let fx = Fixture::new("single");
    let second = Host::serve(&fx.dir);
    assert!(matches!(second, Err(HostError::AlreadyRunning(_))));
    // And the first is none the worse for it.
    assert!(fx.client().list().unwrap().is_empty());
}

#[test]
fn a_client_outlives_its_host_and_reconnects() {
    let mut fx = Fixture::new("reconnect");
    let client = fx.client();
    let woken = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&woken);
    client.set_waker(Arc::new(move || {
        counter.fetch_add(1, Ordering::Relaxed);
    }));
    let info = client.run(&sh("sleep 600")).unwrap();
    let mut stream = client.attach(info.id, 80, 24, false).unwrap();
    let events = client.subscribe().unwrap();
    client.woken();
    let before = woken.load(Ordering::Relaxed);

    fx.stop();
    wait_for("the disconnect", || (!client.is_connected()).then_some(()));
    assert!(matches!(client.list(), Err(ClientError::Disconnected)));
    // The program was ended by the stopping host, or the connection went
    // first; either way the stream is over.
    let end = read_to_end(&mut stream, &mut Vec::new());
    assert!(
        matches!(end, Exit::Disconnected | Exit::Exited { .. }),
        "{end:?}"
    );
    assert!(!events.is_connected());
    assert!(woken.load(Ordering::Relaxed) > before, "the waker is told");
    assert!(client.woken());
    assert!(matches!(client.reconnect(), Err(ClientError::NotRunning)));

    fx.start();
    client.reconnect().unwrap();
    assert!(client.is_connected());
    let list = client.list().unwrap();
    assert_eq!(list.len(), 1);
    assert!(!list[0].state.is_running());
    let fresh = client.run(&sh("echo back; sleep 600")).unwrap();
    let mut stream = client.attach(fresh.id, 80, 24, true).unwrap();
    read_until(&mut stream, &mut Vec::new(), "back");
}

#[test]
fn no_host_and_no_permission_to_start_one() {
    let dir = std::env::temp_dir().join(format!("artalde-host-{}-none", std::process::id()));
    let refused = Client::connect(&dir, Spawn::Never);
    assert!(matches!(refused, Err(ClientError::NotRunning)));
    assert!(!dir.exists(), "asking does not create the state directory");
}

fn raw_request(socket: &mut UnixStream, id: u64, method: &str, params: serde_json::Value) {
    let body = serde_json::json!({ "id": id, "method": method, "params": params });
    let frame = Frame::new(Kind::Request, 0, serde_json::to_vec(&body).unwrap());
    wire::write_frame(socket, &frame).unwrap();
}

const FLOOD_LINE: &str = "0123456789012345678901234567890123456789012345678901234567890123456789";
const FLOOD_LINES: usize = 200_000;

fn flood() -> RunOptions {
    sh(&format!(
        "sleep 0.3; yes {FLOOD_LINE} | head -n {FLOOD_LINES}; echo END; sleep 600"
    ))
}

#[test]
fn a_viewer_that_does_not_read_holds_nobody_up() {
    let fx = Fixture::new("slow");
    let client = fx.client();
    let info = client.run(&flood()).unwrap();
    let produced = FLOOD_LINES * (FLOOD_LINE.len() + 2);

    // A viewer that attaches and then reads nothing at all.
    let mut stuck = UnixStream::connect(fx.host().socket()).unwrap();
    raw_request(
        &mut stuck,
        1,
        "hello",
        serde_json::json!({ "client": "stuck", "version": wire::PROTOCOL_VERSION }),
    );
    raw_request(
        &mut stuck,
        2,
        "attach",
        serde_json::json!({ "terminal": info.id, "cols": 80, "rows": 24, "history": false }),
    );

    // The program gets through all of it regardless, and a viewer that does
    // read sees the end.
    let mut stream = client.attach(info.id, 80, 24, false).unwrap();
    let mut tail = Vec::new();
    wait_for("the end of the flood", || {
        let mut chunk = Vec::new();
        stream.try_read(&mut chunk);
        tail.extend_from_slice(&chunk);
        let keep = tail.len().saturating_sub(64);
        tail.drain(..keep);
        contains(&tail, b"END\r\n").then_some(())
    });
    assert!(client.read(info.id, 2).unwrap().ends_with("END"));

    // When the stuck viewer does start reading, it is not served the
    // megabytes it missed: its backlog was replaced by a replay, which ends
    // where the terminal is now.
    stuck
        .set_read_timeout(Some(Duration::from_millis(500)))
        .unwrap();
    let mut output = Vec::new();
    let mut total = 0;
    let mut replies = 0;
    while !contains(&output, b"END\r\n") {
        let frame = wire::read_frame(&mut stuck).unwrap().unwrap();
        match frame.kind {
            Kind::Reply => replies += 1,
            Kind::Output => {
                assert_eq!(replies, 2, "output only after the attach reply");
                total += frame.payload.len();
                if frame.payload.starts_with(b"\x18\x1bc") {
                    output.clear();
                }
                output.extend_from_slice(&frame.payload);
                let keep = output.len().saturating_sub(64);
                output.drain(..keep);
            }
            other => panic!("unexpected {other:?}"),
        }
    }
    assert!(
        total < produced / 2,
        "{total} of {produced} bytes were kept for a viewer that was not reading"
    );
}

#[test]
fn a_stream_nobody_reads_recovers_with_a_replay() {
    let fx = Fixture::new("lag");
    let client = fx.client();
    let info = client.run(&flood()).unwrap();
    // Attached but not read while megabytes go by.
    let mut stream = client.attach(info.id, 80, 24, false).unwrap();
    let watcher = fx.client();
    wait_for("the end of the flood", || {
        watcher
            .read(info.id, 1)
            .unwrap()
            .ends_with("END")
            .then_some(())
    });
    let mut seen = Vec::new();
    read_until(&mut stream, &mut seen, "END\r\n");
    let produced = FLOOD_LINES * (FLOOD_LINE.len() + 2);
    assert!(seen.len() < produced, "{} bytes", seen.len());
    let reset = seen
        .windows(3)
        .rposition(|w| w == b"\x18\x1bc")
        .expect("a reset");
    let text = String::from_utf8_lossy(&seen[reset + 3..]).into_owned();
    assert!(text.starts_with(FLOOD_LINE), "starts on a line");
    assert!(text.ends_with("END\r\n"));
    // And the stream is live again.
    assert_eq!(stream.ended(), None);
    client.kill(info.id).unwrap();
    assert!(matches!(
        read_to_end(&mut stream, &mut seen),
        Exit::Exited { .. }
    ));
}

#[test]
fn twenty_terminals_at_once() {
    let fx = Fixture::new("twenty");
    let client = fx.client();
    let mut streams: Vec<Stream> = (0..20)
        .map(|_| {
            let info = client
                .run(&sh("stty -echo; echo t$ARTALDE_TERMINAL; exec cat"))
                .unwrap();
            client.attach(info.id, 80, 24, true).unwrap()
        })
        .collect();
    let list = client.list().unwrap();
    assert_eq!(list.len(), 20);
    assert!(list.windows(2).all(|w| w[0].id < w[1].id));
    assert!(list.iter().all(|t| t.state.is_running() && t.viewers == 1));
    for stream in &mut streams {
        let id = stream.terminal();
        read_until(stream, &mut Vec::new(), &format!("t{id}\r\n"));
    }
    for stream in &streams {
        stream
            .write(format!("to-{}\n", stream.terminal()).as_bytes())
            .unwrap();
    }
    for stream in &mut streams {
        let id = stream.terminal();
        let mut seen = Vec::new();
        read_until(stream, &mut seen, &format!("to-{id}\r\n"));
        let text = String::from_utf8_lossy(&seen).into_owned();
        assert_eq!(text.matches("to-").count(), 1, "only its own input");
    }
    let pids: Vec<u32> = list.iter().filter_map(|t| t.pid).collect();
    drop(streams);
    drop(client);
    drop(fx);
    for pid in pids {
        wait_dead(pid);
    }
}

#[test]
fn shutdown_is_refused_while_terminals_run() {
    let mut fx = Fixture::new("shutdown");
    let client = fx.client();
    let info = client.run(&sh("sleep 600")).unwrap();
    let refused = client.shutdown(false);
    assert!(matches!(refused, Err(ClientError::Host(_))), "{refused:?}");
    // Refusing changed nothing.
    assert!(client.get(info.id).unwrap().state.is_running());
    client.run(&sh("sleep 600")).unwrap();

    assert!(matches!(
        client.remove(info.id, false),
        Err(ClientError::Host(_))
    ));
    client.remove(info.id, true).unwrap();
    wait_dead(info.pid.unwrap());
    assert_eq!(client.list().unwrap().len(), 1);

    client.shutdown(true).unwrap();
    assert!(!client.is_connected());
    fx.host().wait();
    assert!(!fx.host().socket().exists());
    // The lock is free again.
    fx.stop();
    fx.start();
    assert_eq!(fx.client().list().unwrap().len(), 1);
}

/// What the host does with `bytes` from a client: it must close that
/// connection and go on serving the others.
fn host_drops(socket: &Path, bytes: &[u8], close_after: bool) {
    let mut raw = UnixStream::connect(socket).unwrap();
    raw.set_read_timeout(Some(PATIENCE)).unwrap();
    raw.write_all(bytes).unwrap();
    if close_after {
        raw.shutdown(std::net::Shutdown::Write).unwrap();
    }
    let mut rest = Vec::new();
    // End of file, not a timeout: the host hung up.
    raw.read_to_end(&mut rest).unwrap();
    assert!(rest.is_empty(), "{rest:?}");
}

#[test]
fn malformed_frames_cost_the_sender_its_connection_only() {
    let fx = Fixture::new("malformed");
    let client = fx.client();
    let socket = fx.host().socket();

    // An unknown kind.
    host_drops(socket, &[99, 0, 0, 0, 0, 0, 0, 0, 0], false);
    // A length over the limit.
    let mut oversized = vec![1, 0, 0, 0, 0];
    oversized.extend_from_slice(&u32::MAX.to_le_bytes());
    host_drops(socket, &oversized, false);
    // A frame cut short, in the header and in the payload.
    host_drops(socket, &[1, 0, 0], true);
    host_drops(socket, &[1, 0, 0, 0, 0, 10, 0, 0, 0, b'{'], true);
    // A request that is not JSON, and a frame only the host may send.
    host_drops(
        socket,
        &[1, 0, 0, 0, 0, 3, 0, 0, 0, b'n', b'o', b'!'],
        false,
    );
    host_drops(socket, &[10, 1, 0, 0, 0, 0, 0, 0, 0], false);

    // Bad parameters and unknown methods are answered, not punished.
    let mut raw = UnixStream::connect(socket).unwrap();
    raw.set_read_timeout(Some(PATIENCE)).unwrap();
    raw_request(&mut raw, 7, "frobnicate", serde_json::Value::Null);
    raw_request(&mut raw, 8, "get", serde_json::json!({ "terminal": "x" }));
    raw_request(&mut raw, 9, "hello", serde_json::json!({ "version": 999 }));
    raw_request(&mut raw, 10, "attach", serde_json::json!({ "terminal": 5 }));
    for id in 7..=10 {
        let frame = wire::read_frame(&mut raw).unwrap().unwrap();
        let reply: serde_json::Value = serde_json::from_slice(&frame.payload).unwrap();
        assert_eq!(reply["id"], id);
        assert!(reply["error"].is_string(), "{reply}");
    }
    // Input for a terminal that is not attached goes nowhere.
    wire::write_frame(&mut raw, &Frame::new(Kind::Input, 5, b"x".to_vec())).unwrap();
    wire::write_frame(&mut raw, &Frame::new(Kind::Resize, 5, vec![1, 2, 3])).unwrap();
    wire::write_frame(&mut raw, &Frame::new(Kind::Detach, 5, Vec::new())).unwrap();
    raw_request(&mut raw, 11, "list", serde_json::Value::Null);
    let frame = wire::read_frame(&mut raw).unwrap().unwrap();
    let reply: serde_json::Value = serde_json::from_slice(&frame.payload).unwrap();
    assert_eq!(reply["id"], 11);

    assert!(client.list().unwrap().is_empty());
    assert!(matches!(client.get(1), Err(ClientError::Host(_))));
    assert!(matches!(
        client.attach(1, 80, 24, false),
        Err(ClientError::Host(_))
    ));
    assert!(matches!(
        client.run(&RunOptions {
            argv: vec!["/no/such/program".into()],
            ..RunOptions::default()
        }),
        Err(ClientError::Host(_))
    ));
    assert!(matches!(
        client.run(&RunOptions {
            cwd: Some("/no/such/dir".into()),
            ..RunOptions::default()
        }),
        Err(ClientError::Host(_))
    ));
    assert!(client.list().unwrap().is_empty());
}
