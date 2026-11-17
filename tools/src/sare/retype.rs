use crate::kg::{self, Ontology, Paths, Row};
use crate::sare::Counter;
use regex::Regex;
use std::collections::HashMap;
use std::sync::OnceLock;

fn date_suffix() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"-(?:19|20)\d{2}(?:-\d{2}){0,2}$").unwrap())
}

fn trailing_date() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?:19|20)\d{2}(?:-\d{2}){0,2}$").unwrap())
}

fn key_line() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^(links|edges):\s*(\[\])?\s*$").unwrap())
}

fn item_line() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^\s+-\s").unwrap())
}

fn page_date(id: &str) -> String {
    trailing_date().find(id).map(|m| m.as_str().to_string()).unwrap_or_default()
}

pub enum Verdict {
    Typed(&'static str, &'static str),
    Drop(&'static str),
    Unclear(String),
}

pub fn rule(src_id: &str, src_type: &str, dst_id: &str, dst_type: &str, nodes: &HashMap<String, Row>) -> Verdict {
    if src_type == "decision" {
        let stem = date_suffix().replace(src_id, "").to_string();
        if matches!(dst_type, "project" | "concept") {
            if stem == dst_id || stem.starts_with(&format!("{dst_id}-")) {
                return Verdict::Typed("DECIDES", "decision slug is prefixed by its subject");
            }
            return Verdict::Typed("AFFECTS", "a decision naming a project it is not about constrains it");
        }
        if dst_type == "source" {
            return Verdict::Typed("EVIDENCED_BY", "a decision pointing at raw material is its evidence");
        }
        if dst_type == "decision" {
            let target_status = nodes
                .get(dst_id)
                .map(|n| kg::get(n, "status").to_string())
                .unwrap_or_default();
            let same_subject = date_suffix().replace(dst_id, "") == stem;
            let dst_date = page_date(dst_id);
            let newer = !dst_date.is_empty() && page_date(src_id) > dst_date;
            if target_status.starts_with("superseded") && same_subject && newer {
                return Verdict::Typed("SUPERSEDES", "target is marked superseded, same subject, later date");
            }
        }
        return Verdict::Unclear(format!("decision -> {dst_type} needs judgment"));
    }

    if src_type == "concept" {
        if matches!(dst_type, "project" | "decision") {
            return Verdict::Typed("APPLIES_TO", "a concept page listing a project is where it applies");
        }
        if dst_type == "source" {
            return Verdict::Typed("EVIDENCED_BY", "raw material behind the concept");
        }
        return Verdict::Unclear(format!("concept -> {dst_type} needs judgment"));
    }

    if src_type == "project" {
        if dst_type == "source" {
            return Verdict::Typed("EVIDENCED_BY", "raw material behind the project");
        }
        if dst_type == "decision" {
            return Verdict::Drop("covered by the decision's own DECIDES edge");
        }
        return Verdict::Unclear(format!("project -> {dst_type} needs judgment (DEPENDS_ON? PART_OF?)"));
    }

    if src_type == "source" && matches!(dst_type, "project" | "concept" | "decision") {
        return Verdict::Drop("a source is immutable evidence; the consumer owns the edge");
    }

    Verdict::Unclear(format!("{src_type} -> {dst_type} has no rule"))
}

pub fn run(paths: &Paths, _ont: &Ontology, only: Option<&str>, apply: bool) -> i32 {
    let nodes: HashMap<String, Row> = kg::read_tsv(&paths.nodes_tsv())
        .into_iter()
        .map(|r| (kg::get(&r, "id").to_string(), r))
        .collect();

    let mut converted = Counter::new();
    let mut unresolved = Counter::new();
    let mut changed_pages = 0;

    for (nid, ntype, path) in kg::wiki_pages(&paths.root) {
        if only.is_some_and(|target| nid != target) {
            continue;
        }
        let fm = kg::frontmatter(&path);
        let links = fm.list("links");
        if links.is_empty() {
            continue;
        }
        let mut keep_links: Vec<String> = Vec::new();
        let mut new_edges: Vec<String> = Vec::new();
        for link in &links {
            let dst = kg::slug(link);
            let dst_type = nodes.get(&dst).map(|n| kg::get(n, "type").to_string()).unwrap_or_default();
            if dst_type.is_empty() {
                keep_links.push(link.clone());
                unresolved.add("broken target");
                continue;
            }
            match rule(&nid, &ntype, &dst, &dst_type, &nodes) {
                Verdict::Drop(_) => converted.add("(dropped as redundant)"),
                Verdict::Typed(rel, _) => {
                    new_edges.push(format!("{rel} {dst}"));
                    converted.add(rel);
                }
                Verdict::Unclear(why) => {
                    keep_links.push(link.clone());
                    unresolved.add(&why);
                }
            }
        }

        if new_edges.is_empty() && keep_links.len() == links.len() {
            continue;
        }
        changed_pages += 1;
        if !apply {
            println!("{nid} ({ntype}):");
            for e in &new_edges {
                println!("    + {e}");
            }
            for l in &keep_links {
                println!("      keep links: {}", kg::slug(l));
            }
            continue;
        }

        let mut existing = fm.list("edges");
        for e in &new_edges {
            if !existing.contains(e) {
                existing.push(e.clone());
            }
        }
        let lines: Vec<&str> = fm.block.split('\n').collect();
        let mut out: Vec<String> = Vec::new();
        let mut i = 0;
        while i < lines.len() {
            if key_line().is_match(lines[i]) {
                i += 1;
                while i < lines.len() && item_line().is_match(lines[i]) {
                    i += 1;
                }
                continue;
            }
            out.push(lines[i].to_string());
            i += 1;
        }
        if !existing.is_empty() {
            out.push("edges:".into());
            out.extend(existing.iter().map(|e| format!("  - \"{e}\"")));
        }
        if !keep_links.is_empty() {
            out.push("links:".into());
            out.extend(keep_links.iter().map(|l| format!("  - \"[[{}]]\"", kg::slug(l))));
        }
        let kept: Vec<String> = out.into_iter().filter(|x| !x.trim().is_empty()).collect();
        let text = format!("---\n{}\n---\n{}", kept.join("\n"), fm.body);
        if let Err(e) = std::fs::write(&path, text) {
            eprintln!("sare: cannot rewrite {}: {e}", paths.rel(&path));
        }
    }

    println!("\n{} {changed_pages} pages", if apply { "rewrote" } else { "would rewrite" });
    println!("typed: {}", py_dict(&converted));
    println!("left untyped: {}", py_dict(&unresolved));
    0
}

fn py_dict(counter: &Counter) -> String {
    let items: Vec<String> = counter
        .most_common_insertion_order()
        .into_iter()
        .map(|(k, v)| format!("'{k}': {v}"))
        .collect();
    format!("{{{}}}", items.join(", "))
}
