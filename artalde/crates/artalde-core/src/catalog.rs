//! Titles and states for PRs and tickets, from what the wiki's pipeline
//! already fetched: `<wiki>/graph/raw/{watch,github,linear}.json` (zaindu and
//! `sare build`). No network: an id the wiki has not seen keeps a bare link.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::Deserialize;

use crate::model::{Resource, ResourceKind};

/// Graph files larger than this are skipped.
const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;
const RAW_FILES: [&str; 4] = [
    "github.json",
    "watch.json",
    "linear.json",
    "github-issues.json",
];
/// Linear workspace used for links when the wiki has none.
const DEFAULT_LINEAR: &str = "https://linear.app/issue";

#[derive(Debug, Deserialize)]
struct RawGraph {
    #[serde(default)]
    nodes: Vec<RawNode>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct RawNode {
    id: String,
    #[serde(rename = "type")]
    kind: String,
    title: String,
    status: String,
    state: String,
    url: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Entry {
    title: String,
    state: String,
    url: String,
}

#[derive(Debug, Default)]
pub struct Catalog {
    prs: HashMap<String, Entry>,
    tickets: HashMap<String, Entry>,
    /// `https://linear.app/<workspace>`, learnt from any issue URL.
    linear_base: Option<String>,
    stamps: Vec<(PathBuf, SystemTime)>,
}

impl Catalog {
    /// Re-read the raw graph files if any changed since the last load.
    pub fn refresh(&mut self, wiki: &Path) {
        let raw = wiki.join("graph/raw");
        let stamps: Vec<(PathBuf, SystemTime)> = RAW_FILES
            .iter()
            .map(|f| raw.join(f))
            .filter_map(|p| Some((p.clone(), fs::metadata(&p).ok()?.modified().ok()?)))
            .collect();
        if stamps == self.stamps {
            return;
        }
        let mut next = Catalog {
            stamps,
            ..Catalog::default()
        };
        // Older first: watch.json (zaindu, every 5 min) overrides github.json.
        for (path, _) in &next.stamps.clone() {
            if fs::metadata(path).map_or(true, |m| m.len() > MAX_FILE_BYTES) {
                continue;
            }
            let Ok(body) = fs::read(path) else { continue };
            let Ok(graph) = serde_json::from_slice::<RawGraph>(&body) else {
                continue;
            };
            for n in graph.nodes {
                next.add(n);
            }
        }
        *self = next;
    }

    fn add(&mut self, n: RawNode) {
        let entry = || Entry {
            title: n.title.clone(),
            state: if n.state.is_empty() {
                n.status.clone()
            } else {
                n.state.clone()
            },
            url: n.url.clone(),
        };
        match n.kind.as_str() {
            "pr" => {
                if let Some(id) = n.id.strip_prefix("gh:") {
                    self.prs.insert(id.to_owned(), entry());
                }
            }
            "issue" if n.id.starts_with("gh:") => {
                self.tickets.insert(n.id.clone(), entry());
            }
            "issue" => {
                if self.linear_base.is_none() {
                    self.linear_base = n
                        .url
                        .find("/issue/")
                        .and_then(|i| n.url.get(..i))
                        .map(str::to_owned);
                }
                self.tickets.insert(n.id.clone(), entry());
            }
            _ => {}
        }
    }

    /// A resource for `id`, with what the wiki knows about it.
    pub fn resolve(&self, kind: ResourceKind, id: &str, mentions: u32) -> Resource {
        let known = match kind {
            ResourceKind::Pr => self.prs.get(id),
            ResourceKind::Ticket => self.tickets.get(id),
        };
        let fallback = || match kind {
            ResourceKind::Pr => match id.split_once('#') {
                Some((repo, n)) => format!("https://github.com/{repo}/pull/{n}"),
                None => String::new(),
            },
            ResourceKind::Ticket if id.starts_with("gh:") && id.contains("/issues/") => {
                format!("https://github.com/{}", id.trim_start_matches("gh:"))
            }
            ResourceKind::Ticket => format!(
                "{}/issue/{id}",
                self.linear_base
                    .as_deref()
                    .unwrap_or(DEFAULT_LINEAR)
                    .trim_end_matches("/issue")
            ),
        };
        Resource {
            kind,
            id: id.to_owned(),
            title: known.map(|e| e.title.clone()).filter(|t| !t.is_empty()),
            state: known.map(|e| e.state.clone()).filter(|s| !s.is_empty()),
            url: known
                .map(|e| e.url.clone())
                .filter(|u| !u.is_empty())
                .unwrap_or_else(fallback),
            mentions,
        }
    }
}

/// Linear team keys from `<wiki>/os.yaml` (`linear: teams: [THU, ENG]`).
pub fn linear_teams(wiki: &Path) -> Vec<String> {
    let Ok(text) = fs::read_to_string(wiki.join("os.yaml")) else {
        return Vec::new();
    };
    let Some(line) = text.lines().find(|l| l.trim_start().starts_with("teams:")) else {
        return Vec::new();
    };
    line.split_once(':')
        .map(|(_, v)| v)
        .unwrap_or_default()
        .trim()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .split(',')
        .map(|t| t.trim().trim_matches(['"', '\'']).to_owned())
        .filter(|t| {
            !t.is_empty()
                && t.len() <= 10
                && t.chars()
                    .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_from_raw_graph() {
        let wiki = std::env::temp_dir().join(format!("artalde-catalog-{}", std::process::id()));
        fs::create_dir_all(wiki.join("graph/raw")).unwrap();
        fs::write(
            wiki.join("graph/raw/watch.json"),
            r#"{"nodes":[{"id":"gh:join-com/data#7939","type":"pr","title":"Dedup names","status":"open+red","url":"https://github.com/join-com/data/pull/7939"}]}"#,
        )
        .unwrap();
        fs::write(
            wiki.join("graph/raw/linear.json"),
            r#"{"nodes":[{"id":"THU-4701","type":"issue","title":"CV fidelity","state":"In Progress","url":"https://linear.app/join-com/issue/THU-4701/cv"}]}"#,
        )
        .unwrap();
        fs::write(
            wiki.join("os.yaml"),
            "me:\n  github: x\nlinear:\n  teams: [THU, ENG]\n",
        )
        .unwrap();
        fs::write(
            wiki.join("graph/raw/github-issues.json"),
            r#"{"nodes":[{"id":"gh:acme/core/issues/12","type":"issue","title":"Improve agents","state":"Open","url":"https://github.com/acme/core/issues/12"}]}"#,
        ).unwrap();
        let mut c = Catalog::default();
        c.refresh(&wiki);
        let pr = c.resolve(ResourceKind::Pr, "join-com/data#7939", 3);
        assert_eq!(
            (pr.title.as_deref(), pr.state.as_deref(), pr.short()),
            (Some("Dedup names"), Some("open+red"), "data#7939".into())
        );
        let t = c.resolve(ResourceKind::Ticket, "THU-4701", 1);
        assert_eq!(t.state.as_deref(), Some("In Progress"));
        let unknown = c.resolve(ResourceKind::Ticket, "ENG-9", 1);
        assert_eq!(unknown.url, "https://linear.app/join-com/issue/ENG-9");
        assert_eq!(
            c.resolve(ResourceKind::Pr, "a/b#2", 1).url,
            "https://github.com/a/b/pull/2"
        );
        let issue = c.resolve(ResourceKind::Ticket, "gh:acme/core/issues/12", 1);
        assert_eq!(issue.title.as_deref(), Some("Improve agents"));
        assert_eq!(issue.short(), "core#12");
        assert_eq!(issue.url, "https://github.com/acme/core/issues/12");
        assert_eq!(
            c.resolve(ResourceKind::Ticket, "gh:other/repo/issues/7", 0)
                .url,
            "https://github.com/other/repo/issues/7"
        );
        assert_eq!(linear_teams(&wiki), ["THU", "ENG"]);
    }
}
