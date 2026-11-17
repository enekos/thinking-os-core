use regex::Regex;
use serde_yaml::{Mapping, Value};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

pub const WIKI_DIRS: [(&str, &str); 6] = [
    ("projects", "project"),
    ("concepts", "concept"),
    ("decisions", "decision"),
    ("people", "person"),
    ("sources", "source"),
    ("briefings", "briefing"),
];

fn fm_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?s)\A---\n(.*?)\n---\n").unwrap())
}

fn link_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\A\[\[(.+?)\]\]\z").unwrap())
}

pub struct Front {
    pub map: Mapping,
    pub body: String,
    pub block: String,
    pub block_end: usize,
}

impl Front {
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.map.get(Value::String(key.to_string()))
    }

    pub fn text(&self, key: &str) -> String {
        self.get(key).map(scalar).unwrap_or_default()
    }

    pub fn list(&self, key: &str) -> Vec<String> {
        match self.get(key) {
            Some(Value::Sequence(items)) => items.iter().map(scalar).collect(),
            _ => Vec::new(),
        }
    }
}

pub fn scalar(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Bool(b) => if *b { "True".into() } else { "False".into() },
        Value::Number(n) => n.to_string(),
        Value::Null => String::new(),
        other => serde_yaml::to_string(other).unwrap_or_default().trim().to_string(),
    }
}

pub fn frontmatter(path: &Path) -> Front {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(_) => {
            return Front { map: Mapping::new(), body: String::new(), block: String::new(), block_end: 0 };
        }
    };
    let Some(m) = fm_re().captures(&text) else {
        return Front { map: Mapping::new(), body: text, block: String::new(), block_end: 0 };
    };
    let whole = m.get(0).unwrap();
    let block = m.get(1).unwrap().as_str().to_string();
    let map = match serde_yaml::from_str::<Value>(&block) {
        Ok(Value::Mapping(m)) => m,
        _ => Mapping::new(),
    };
    Front {
        map,
        body: text[whole.end()..].to_string(),
        block,
        block_end: whole.end(),
    }
}

pub fn slug(raw: &str) -> String {
    let mut s = raw.trim().trim_matches('"').trim_matches('\'').to_string();
    if let Some(c) = link_re().captures(&s) {
        s = c.get(1).unwrap().as_str().to_string();
    }
    let s = s.split('|').next().unwrap_or("").split('#').next().unwrap_or("");
    let s = s.rsplit('/').next().unwrap_or("");
    s.strip_suffix(".md").unwrap_or(s).trim().to_string()
}

pub fn wiki_pages(root: &Path) -> Vec<(String, String, PathBuf)> {
    let mut out = Vec::new();
    for (dir, kind) in WIKI_DIRS {
        let base = root.join(dir);
        let Ok(entries) = std::fs::read_dir(&base) else {
            continue;
        };
        let mut names: Vec<String> = entries
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n.ends_with(".md") && !n.starts_with('_'))
            .collect();
        names.sort();
        for name in names {
            out.push((slug(&name), kind.to_string(), base.join(&name)));
        }
    }
    out
}
