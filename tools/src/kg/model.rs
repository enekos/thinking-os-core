use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

pub const NODE_COLS: [&str; 9] = [
    "id", "type", "title", "summary", "status", "state", "updated", "path", "url",
];
pub const EDGE_COLS: [&str; 7] = ["src", "rel", "dst", "since", "prov", "conf", "evidence"];

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Node {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub state: String,
    #[serde(default)]
    pub updated: String,
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub url: String,
}

impl Node {
    pub fn new(id: &str, kind: &str) -> Node {
        Node { id: id.to_string(), kind: kind.to_string(), ..Node::default() }
    }

    pub fn row(&self) -> Vec<String> {
        vec![
            self.id.clone(), self.kind.clone(), self.title.clone(), self.summary.clone(),
            self.status.clone(), self.state.clone(), self.updated.clone(),
            self.path.clone(), self.url.clone(),
        ]
    }

    pub fn field(&self, col: &str) -> &str {
        match col {
            "id" => &self.id,
            "type" => &self.kind,
            "title" => &self.title,
            "summary" => &self.summary,
            "status" => &self.status,
            "state" => &self.state,
            "updated" => &self.updated,
            "path" => &self.path,
            "url" => &self.url,
            _ => "",
        }
    }

    pub fn set(&mut self, col: &str, value: &str) {
        let slot = match col {
            "id" => &mut self.id,
            "type" => &mut self.kind,
            "title" => &mut self.title,
            "summary" => &mut self.summary,
            "status" => &mut self.status,
            "state" => &mut self.state,
            "updated" => &mut self.updated,
            "path" => &mut self.path,
            "url" => &mut self.url,
            _ => return,
        };
        *slot = value.to_string();
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Edge {
    pub src: String,
    pub rel: String,
    pub dst: String,
    #[serde(default)]
    pub since: String,
    #[serde(default = "page_prov")]
    pub prov: String,
    #[serde(default = "full_conf")]
    pub conf: f64,
    #[serde(default)]
    pub evidence: String,
}

fn page_prov() -> String {
    "page".to_string()
}

fn full_conf() -> f64 {
    1.0
}

impl Edge {
    pub fn new(src: &str, rel: &str, dst: &str, since: &str, prov: &str, conf: f64, evidence: &str) -> Edge {
        Edge {
            src: src.to_string(), rel: rel.to_string(), dst: dst.to_string(),
            since: since.to_string(), prov: prov.to_string(), conf,
            evidence: evidence.to_string(),
        }
    }

    pub fn key(&self) -> (String, String, String) {
        (self.src.clone(), self.rel.clone(), self.dst.clone())
    }

    pub fn row(&self) -> Vec<String> {
        vec![
            self.src.clone(), self.rel.clone(), self.dst.clone(), self.since.clone(),
            self.prov.clone(), format!("{:.2}", self.conf), self.evidence.clone(),
        ]
    }
}

pub fn clean(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut in_run = false;
    for c in value.chars() {
        if c == '\t' || c == '\r' || c == '\n' {
            if !in_run {
                out.push(' ');
                in_run = true;
            }
        } else {
            out.push(c);
            in_run = false;
        }
    }
    out.trim().to_string()
}

pub fn write_tsv(path: &Path, cols: &[&str], rows: &[Vec<String>]) -> std::io::Result<()> {
    let mut text = String::new();
    text.push_str(&cols.join("\t"));
    text.push('\n');
    for row in rows {
        let cleaned: Vec<String> = row.iter().map(|c| clean(c)).collect();
        text.push_str(&cleaned.join("\t"));
        text.push('\n');
    }
    super::write_atomic(path, text.as_bytes())
}

pub type Row = BTreeMap<String, String>;

pub fn read_tsv(path: &Path) -> Vec<Row> {
    let text = match fs::read_to_string(path) {
        Ok(t) => t,
        Err(_) => return Vec::new(),
    };
    let mut lines = text.lines();
    let cols: Vec<&str> = match lines.next() {
        Some(h) => h.split('\t').collect(),
        None => return Vec::new(),
    };
    let mut out = Vec::new();
    for line in lines {
        if line.trim().is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.split('\t').collect();
        let mut row = Row::new();
        for (i, col) in cols.iter().enumerate() {
            row.insert((*col).to_string(), parts.get(i).copied().unwrap_or("").to_string());
        }
        out.push(row);
    }
    out
}

pub fn get<'a>(row: &'a Row, col: &str) -> &'a str {
    row.get(col).map(String::as_str).unwrap_or("")
}
