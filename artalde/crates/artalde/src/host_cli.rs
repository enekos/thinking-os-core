//! `artalde host` and `artalde term …`: the terminal host and its terminals
//! from the command line.

use std::collections::BTreeMap;
use std::process::ExitCode;

use anyhow::{Context as _, anyhow, bail};
use artalde_core::now;
use artalde_core::store::state_dir;
use artalde_host::{
    AttachEnd, Client, ClientError, Host, HostError, RunOptions, Spawn, State, TerminalInfo,
    attach_tty,
};

const TERM_USAGE: &str = "\
usage:
  artalde term run [--name N] [--cwd DIR] [--label k=v]… [--] [argv…]
  artalde term ls [--json]
  artalde term read <id> [--lines N]
  artalde term send <id> <text> [--no-submit]
  artalde term kill <id>
  artalde term rm <id> [--force]
  artalde term attach <id>       Ctrl+\\ detaches
  artalde term status
  artalde term stop [--force]
";

/// `artalde host [--foreground]`: be the host, until told to stop. Clients
/// start this detached; `--foreground` says the same for a person at a
/// prompt.
pub fn host(args: &[String]) -> anyhow::Result<ExitCode> {
    if let Some(other) = args.iter().find(|a| *a != "--foreground") {
        bail!("usage: artalde host [--foreground] (unexpected {other:?})");
    }
    match Host::run(&state_dir()) {
        Ok(()) => Ok(ExitCode::SUCCESS),
        Err(HostError::AlreadyRunning(dir)) => {
            eprintln!("artalde host: already running for {}", dir.display());
            Ok(ExitCode::FAILURE)
        }
        Err(e) => Err(e.into()),
    }
}

/// `artalde term <command> …`.
pub fn term(args: &[String]) -> anyhow::Result<ExitCode> {
    let rest = args.get(1..).unwrap_or_default();
    match args.first().map(String::as_str) {
        Some("run") => run(rest),
        Some("ls" | "list") => ls(rest.iter().any(|a| a == "--json")),
        Some("read") => read(rest),
        Some("send") => send(rest),
        Some("kill") => {
            let info = running_host()?.kill(id(rest)?)?;
            println!("{}", state(&info));
            Ok(ExitCode::SUCCESS)
        }
        Some("rm" | "remove") => {
            running_host()?.remove(id(rest)?, rest.iter().any(|a| a == "--force"))?;
            Ok(ExitCode::SUCCESS)
        }
        Some("attach") => attach(rest),
        Some("status") => status(),
        Some("stop") => stop(rest.iter().any(|a| a == "--force")),
        Some("-h" | "--help" | "help") => {
            print!("{TERM_USAGE}");
            Ok(ExitCode::SUCCESS)
        }
        other => {
            match other {
                Some(other) => eprint!("unknown command term {other:?}\n\n{TERM_USAGE}"),
                None => eprint!("{TERM_USAGE}"),
            }
            Ok(ExitCode::from(2))
        }
    }
}

/// The host, started if there is none.
fn any_host() -> anyhow::Result<Client> {
    let exe = std::env::current_exe().context("finding the artalde binary")?;
    Ok(Client::connect(&state_dir(), Spawn::IfNeeded { exe })?)
}

/// The host, if one runs: looking at terminals does not start one.
fn running_host() -> anyhow::Result<Client> {
    match Client::connect(&state_dir(), Spawn::Never) {
        Ok(client) => Ok(client),
        Err(ClientError::NotRunning) => bail!("no host is running (`artalde term run` starts one)"),
        Err(e) => Err(e.into()),
    }
}

fn id(args: &[String]) -> anyhow::Result<u32> {
    let text = args
        .first()
        .filter(|a| !a.starts_with("--"))
        .ok_or_else(|| anyhow!("a terminal id is required"))?;
    text.parse()
        .map_err(|_| anyhow!("{text:?} is not a terminal id"))
}

fn run(args: &[String]) -> anyhow::Result<ExitCode> {
    let mut options = RunOptions::default();
    let mut labels = BTreeMap::new();
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        let mut value = |what: &str| {
            rest.next()
                .cloned()
                .ok_or_else(|| anyhow!("{arg} needs {what}"))
        };
        match arg.as_str() {
            "--name" => options.name = Some(value("a name")?),
            "--cwd" => {
                let dir = value("a directory")?;
                let dir = std::path::absolute(&dir).with_context(|| format!("--cwd {dir}"))?;
                options.cwd = Some(
                    dir.into_os_string()
                        .into_string()
                        .map_err(|_| anyhow!("--cwd is not UTF-8"))?,
                );
            }
            "--label" => {
                let label = value("k=v")?;
                let (key, val) = label
                    .split_once('=')
                    .ok_or_else(|| anyhow!("--label {label:?} is not k=v"))?;
                labels.insert(key.to_owned(), val.to_owned());
            }
            "--" => {
                options.argv = rest.cloned().collect();
                break;
            }
            flag if flag.starts_with("--") => bail!("unknown option {flag}\n\n{TERM_USAGE}"),
            _ => {
                options.argv = std::iter::once(arg).chain(rest).cloned().collect();
                break;
            }
        }
    }
    options.labels = labels;
    if options.cwd.is_none() {
        // Where the command was typed, not where the host happens to be.
        options.cwd = std::env::current_dir()
            .ok()
            .and_then(|dir| dir.into_os_string().into_string().ok());
    }
    let info = any_host()?.run(&options)?;
    println!("{}", info.id);
    Ok(ExitCode::SUCCESS)
}

fn state(info: &TerminalInfo) -> String {
    match &info.state {
        State::Running => match &info.status {
            Some(status) => format!("{:?}", status.state).to_lowercase(),
            None => "running".to_owned(),
        },
        State::Exited { lost: true, .. } => "lost".to_owned(),
        State::Exited {
            signal: Some(signal),
            ..
        } => format!("signal {signal}"),
        State::Exited {
            code: Some(code), ..
        } => format!("exit {code}"),
        State::Exited { .. } => "exited".to_owned(),
    }
}

fn age(seconds: u64) -> String {
    match seconds {
        0..60 => format!("{seconds}s"),
        60..3600 => format!("{}m", seconds / 60),
        3600..86_400 => format!("{}h", seconds / 3600),
        _ => format!("{}d", seconds / 86_400),
    }
}

fn ls(json: bool) -> anyhow::Result<ExitCode> {
    let list = match Client::connect(&state_dir(), Spawn::Never) {
        Ok(client) => client.list()?,
        Err(ClientError::NotRunning) => Vec::new(),
        Err(e) => return Err(e.into()),
    };
    if json {
        println!("{}", serde_json::to_string_pretty(&list)?);
        return Ok(ExitCode::SUCCESS);
    }
    if list.is_empty() {
        return Ok(ExitCode::SUCCESS);
    }
    println!(
        "{:>4}  {:16} {:10} {:>7} {:>4}  {:10} CWD",
        "ID", "NAME", "STATE", "SIZE", "AGE", "RUNNING"
    );
    let now = now();
    for t in &list {
        println!(
            "{:>4}  {:16} {:10} {:>7} {:>4}  {:10} {}",
            t.id,
            t.name,
            state(t),
            format!("{}x{}", t.cols, t.rows),
            age(now.saturating_sub(t.created)),
            t.foreground.as_deref().unwrap_or("-"),
            t.cwd
        );
    }
    Ok(ExitCode::SUCCESS)
}

fn read(args: &[String]) -> anyhow::Result<ExitCode> {
    let lines = match crate::flag(args, "--lines") {
        Some(n) => n
            .parse()
            .map_err(|_| anyhow!("--lines {n:?} is not a number"))?,
        None => 100,
    };
    let text = running_host()?.read(id(args)?, lines)?;
    if !text.is_empty() {
        println!("{text}");
    }
    Ok(ExitCode::SUCCESS)
}

fn send(args: &[String]) -> anyhow::Result<ExitCode> {
    let terminal = id(args)?;
    let submit = !args.iter().any(|a| a == "--no-submit");
    let text = args
        .get(1..)
        .unwrap_or_default()
        .iter()
        .find(|a| *a != "--no-submit")
        .ok_or_else(|| anyhow!("usage: artalde term send <id> <text> [--no-submit]"))?;
    running_host()?.send(terminal, text, submit)?;
    Ok(ExitCode::SUCCESS)
}

fn attach(args: &[String]) -> anyhow::Result<ExitCode> {
    let terminal = id(args)?;
    let client = running_host()?;
    Ok(match attach_tty(&client, terminal, true)? {
        AttachEnd::Detached => {
            eprintln!("[detached from terminal {terminal}]");
            ExitCode::SUCCESS
        }
        AttachEnd::Exited { code, signal } => {
            match (code, signal) {
                (_, Some(signal)) => eprintln!("[terminal {terminal} ended by signal {signal}]"),
                (Some(code), None) => eprintln!("[terminal {terminal} exited {code}]"),
                (None, None) => eprintln!("[terminal {terminal} ended]"),
            }
            match code.map(u8::try_from) {
                Some(Ok(code)) => ExitCode::from(code),
                _ => ExitCode::FAILURE,
            }
        }
        AttachEnd::Disconnected => {
            eprintln!("[the host went away]");
            ExitCode::FAILURE
        }
        AttachEnd::Signalled(signal) => {
            ExitCode::from(u8::try_from(128 + signal).unwrap_or(u8::MAX))
        }
    })
}

fn status() -> anyhow::Result<ExitCode> {
    let dir = state_dir();
    let client = match Client::connect(&dir, Spawn::Never) {
        Ok(client) => client,
        Err(ClientError::NotRunning) => {
            println!("not running ({})", dir.display());
            return Ok(ExitCode::FAILURE);
        }
        Err(e) => return Err(e.into()),
    };
    let list = client.list()?;
    let running = list.iter().filter(|t| t.state.is_running()).count();
    match client.host() {
        Some(host) => println!(
            "running: pid {}, up {}, protocol {}",
            host.pid,
            age(now().saturating_sub(host.started)),
            host.version
        ),
        None => println!("running"),
    }
    println!("socket: {}", artalde_host::socket_path(&dir).display());
    println!(
        "terminals: {running} running, {} finished",
        list.len() - running
    );
    Ok(ExitCode::SUCCESS)
}

fn stop(force: bool) -> anyhow::Result<ExitCode> {
    let client = match Client::connect(&state_dir(), Spawn::Never) {
        Ok(client) => client,
        Err(ClientError::NotRunning) => {
            println!("not running");
            return Ok(ExitCode::SUCCESS);
        }
        Err(e) => return Err(e.into()),
    };
    if !force {
        let running = client
            .list()?
            .iter()
            .filter(|t| t.state.is_running())
            .count();
        if running > 0 {
            bail!("{running} terminal(s) still running; `artalde term stop --force` ends them");
        }
    }
    client.shutdown(force)?;
    println!("stopped");
    Ok(ExitCode::SUCCESS)
}
