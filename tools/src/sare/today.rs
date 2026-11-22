//! Deterministic, bounded briefing inputs. Collection never writes wiki pages.
use crate::kg::{self, Paths};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

const MAX_BYTES: u64 = 32 * 1024;
const MAX_PROJECTS: usize = 100;

fn excerpt(path: &Path) -> Value {
    let result = fs::File::open(path).and_then(|file| {
        let mut bytes = Vec::new();
        file.take(MAX_BYTES + 1).read_to_end(&mut bytes)?;
        let truncated = bytes.len() > MAX_BYTES as usize;
        bytes.truncate(MAX_BYTES as usize);
        Ok(json!({"text": String::from_utf8_lossy(&bytes), "truncated": truncated}))
    });
    match result {
        Ok(value) => value,
        Err(error) => json!({"error": error.to_string()}),
    }
}

fn markdown_files(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<_> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "md"))
        .collect();
    files.sort();
    files
}

pub fn collect(paths: &Paths) -> Value {
    let mut warnings = Vec::new();
    let mut projects = Vec::new();
    let mut repos = BTreeSet::new();
    for file in markdown_files(&paths.root.join("projects")) {
        if fs::metadata(&file).is_ok_and(|m| m.len() > MAX_BYTES) {
            warnings.push(format!(
                "{} exceeds the project input limit; skipped",
                paths.rel(&file)
            ));
            continue;
        }
        let fm = kg::frontmatter(&file);
        if fm.text("status") != "active" {
            continue;
        }
        if projects.len() == MAX_PROJECTS {
            warnings.push("active projects truncated at 100".to_string());
            break;
        }
        let raw = fm.text("path");
        let expanded = raw
            .strip_prefix("~/")
            .map(|tail| PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(tail))
            .unwrap_or_else(|| PathBuf::from(&raw));
        if !raw.is_empty() {
            let repo = if expanded.is_absolute() {
                expanded
            } else {
                paths.root.join(expanded)
            };
            if let Some(top) = kg::run(&["git", "rev-parse", "--show-toplevel"], Some(&repo), 10) {
                let top = top.trim();
                if !top.is_empty() && Some(top) != std::env::var("HOME").ok().as_deref() {
                    repos.insert(top.to_string());
                }
            } else {
                warnings.push(format!(
                    "cannot resolve repository for {}",
                    paths.rel(&file)
                ));
            }
        }
        projects.push(json!({"page": paths.rel(&file), "title": fm.text("title"),
            "summary": fm.text("summary"), "updated": fm.text("updated"), "path": raw}));
    }
    let git: Vec<_> = repos.into_iter().map(|repo| {
        let dir = Path::new(&repo);
        let status = kg::run(&["git", "status", "--short", "--untracked-files=no"], Some(dir), 10);
        let recent = kg::run(&["git", "log", "--oneline", "-5", "--since=7 days ago"], Some(dir), 10);
        json!({"repo": repo, "status": status.map(|s| s.lines().take(15).collect::<Vec<_>>().join("\n")),
            "recent": recent, "note": "status limited to 15 lines; untracked files excluded"})
    }).collect();
    let mut proposals = Vec::new();
    let mut proposals_truncated = false;
    let inbox = markdown_files(&paths.root.join("inbox/events"));
    for file in inbox.iter().rev().take(30) {
        let value = excerpt(file);
        if let Some(text) = value["text"].as_str() {
            for line in text.lines().filter(|l| l.starts_with("- [ ] ")) {
                if proposals.len() == 100 {
                    proposals_truncated = true;
                    break;
                }
                proposals.push(json!({"file": paths.rel(file), "text": line}));
            }
        }
        if value["truncated"] == true {
            warnings.push(format!("{} truncated", paths.rel(file)));
        }
    }
    if proposals_truncated {
        warnings.push("proposal output capped at 100; expand inbox files when needed".into());
    }
    let briefings = markdown_files(&paths.root.join("briefings"));
    let latest = briefings
        .last()
        .map(|file| json!({"file": paths.rel(file), "content": excerpt(file)}));
    let log = excerpt(&paths.root.join("log.md"));
    // Read the tail independently so old log entries cannot hide the latest five.
    let log_tail = fs::File::open(paths.root.join("log.md")).and_then(|mut file| {
        use std::io::{Seek, SeekFrom};
        let len = file.metadata()?.len();
        file.seek(SeekFrom::Start(len.saturating_sub(MAX_BYTES)))?;
        let mut bytes = Vec::new();
        file.take(MAX_BYTES).read_to_end(&mut bytes)?;
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    });
    let entries: Vec<_> = log_tail
        .unwrap_or_default()
        .lines()
        .filter(|l| l.starts_with("## ["))
        .map(str::to_string)
        .rev()
        .take(5)
        .collect();
    let views: serde_json::Map<String, Value> = ["work", "projects", "health"]
        .into_iter()
        .map(|name| {
            (
                name.to_string(),
                excerpt(&paths.views().join(format!("{name}.md"))),
            )
        })
        .collect();
    let freshness: serde_json::Map<String, Value> =
        ["pages", "git", "github", "github-issues", "linear", "watch"]
            .into_iter()
            .map(|name| {
                #[derive(serde::Deserialize)]
                struct Stamp {
                    extracted_at: String,
                }
                let stamp = fs::File::open(paths.raw().join(format!("{name}.json")))
                    .ok()
                    .and_then(|file| {
                        serde_json::from_reader::<_, Stamp>(std::io::BufReader::new(
                            file.take(64 * 1024 * 1024),
                        ))
                        .ok()
                    })
                    .map(|stamp| stamp.extracted_at);
                (name.to_string(), json!(stamp))
            })
            .collect();
    json!({"collected_at": crate::clock::stamp(), "root": paths.root,
        "source_freshness": freshness, "views": views, "active_projects": projects,
        "git": git, "proposals": proposals, "proposals_truncated": proposals_truncated, "proposal_window": "latest 30 inbox files, at most 100 items",
        "last_log_entries_newest_first": entries, "log_error": log.get("error"),
        "latest_briefing": latest, "warnings": warnings,
        "next": "Read shortlisted project pages for Next up; rank priorities and write the briefing. Collection makes no remote requests."})
}
