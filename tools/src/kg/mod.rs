pub mod model;
pub mod ontology;
pub mod page;

pub use model::{Edge, Node, Row, clean, get, read_tsv, write_tsv, EDGE_COLS, NODE_COLS};
pub use ontology::Ontology;
pub use page::{frontmatter, slug, wiki_pages, Front};

use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct Paths {
    pub root: PathBuf,
}

impl Paths {
    pub fn resolve(explicit: Option<&str>) -> Paths {
        if let Some(p) = explicit {
            return Paths { root: PathBuf::from(p) };
        }
        for var in ["THINKING_OS_ROOT", "ZAINDU_ROOT"] {
            if let Ok(p) = std::env::var(var) {
                if !p.is_empty() {
                    return Paths { root: PathBuf::from(p) };
                }
            }
        }
        let mut here = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        loop {
            if here.join("graph/ontology.yaml").exists() {
                return Paths { root: here };
            }
            if !here.pop() {
                break;
            }
        }
        Paths {
            root: PathBuf::from(std::env::var("HOME").unwrap_or_default()).join("thinking-os"),
        }
    }

    pub fn graph(&self) -> PathBuf {
        self.root.join("graph")
    }
    pub fn raw(&self) -> PathBuf {
        self.root.join("graph/raw")
    }
    pub fn views(&self) -> PathBuf {
        self.root.join("graph/views")
    }
    pub fn ontology(&self) -> PathBuf {
        self.root.join("graph/ontology.yaml")
    }
    pub fn nodes_tsv(&self) -> PathBuf {
        self.root.join("graph/nodes.tsv")
    }
    pub fn edges_tsv(&self) -> PathBuf {
        self.root.join("graph/edges.tsv")
    }
    pub fn wiki_nodes_tsv(&self) -> PathBuf {
        self.root.join("graph/wiki-nodes.tsv")
    }
    pub fn wiki_edges_tsv(&self) -> PathBuf {
        self.root.join("graph/wiki-edges.tsv")
    }
    pub fn config(&self) -> PathBuf {
        self.root.join("os.yaml")
    }
    /// The owner's GitHub login as a person id: `$THINKING_OS_GITHUB`, else
    /// `me.github` in `os.yaml`. Decides what "mine" means in the views.
    pub fn me(&self) -> Option<String> {
        let from_env = std::env::var("THINKING_OS_GITHUB").ok().filter(|v| !v.trim().is_empty());
        let login = from_env.or_else(|| {
            let text = std::fs::read_to_string(self.config()).ok()?;
            let doc: serde_yaml::Value = serde_yaml::from_str(&text).ok()?;
            doc.get("me")?.get("github")?.as_str().map(str::to_string)
        })?;
        let id = login.trim().to_ascii_lowercase();
        (!id.is_empty()).then_some(id)
    }
    pub fn rel(&self, path: &Path) -> String {
        path.strip_prefix(&self.root).unwrap_or(path).to_string_lossy().to_string()
    }
}

/// Write through a temp file in the same directory, then rename. Two processes
/// share this directory — the pipeline and the zaindu poller — and a plain
/// `fs::write` let fusion read a half-written `raw/watch.json` and silently drop
/// the whole source.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension(format!(
        "tmp{}",
        std::process::id()
    ));
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)
}

pub fn run_capture(cmd: &mut Command, timeout: Duration) -> Result<String, String> {
    let mut child = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("cannot spawn: {e}"))?;

    let mut out_pipe = child.stdout.take();
    let mut err_pipe = child.stderr.take();
    let drain_out = std::thread::spawn(move || {
        let mut buf = String::new();
        if let Some(p) = out_pipe.as_mut() {
            let _ = p.read_to_string(&mut buf);
        }
        buf
    });
    let drain_err = std::thread::spawn(move || {
        let mut buf = String::new();
        if let Some(p) = err_pipe.as_mut() {
            let _ = p.read_to_string(&mut buf);
        }
        buf
    });

    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let out = drain_out.join().unwrap_or_default();
                let err = drain_err.join().unwrap_or_default();
                if status.success() {
                    return Ok(out);
                }
                return Err(err.lines().take(3).collect::<Vec<_>>().join(" ").trim().to_string());
            }
            Ok(None) => {
                if started.elapsed() > timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = drain_out.join();
                    let _ = drain_err.join();
                    return Err(format!("timed out after {}s", timeout.as_secs()));
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => return Err(format!("wait failed: {e}")),
        }
    }
}

pub fn run(args: &[&str], cwd: Option<&Path>, timeout_secs: u64) -> Option<String> {
    let mut cmd = Command::new(args[0]);
    cmd.args(&args[1..]);
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    run_capture(&mut cmd, Duration::from_secs(timeout_secs)).ok()
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Raw {
    #[serde(default)]
    pub extracted_at: String,
    #[serde(default)]
    pub nodes: Vec<Node>,
    #[serde(default)]
    pub edges: Vec<Edge>,
}

pub fn dump_raw(paths: &Paths, name: &str, nodes: &[Node], edges: &[Edge]) -> std::io::Result<()> {
    std::fs::create_dir_all(paths.raw())?;
    let payload = Raw {
        extracted_at: crate::clock::stamp(),
        nodes: nodes.to_vec(),
        edges: edges.to_vec(),
    };
    write_atomic(
        &paths.raw().join(format!("{name}.json")),
        serde_json::to_string_pretty(&payload).unwrap_or_default().as_bytes(),
    )
}

pub fn load_raw(paths: &Paths, name: &str) -> Raw {
    std::fs::read_to_string(paths.raw().join(format!("{name}.json")))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}
