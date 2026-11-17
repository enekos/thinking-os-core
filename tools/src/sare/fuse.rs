use crate::kg::{self, Edge, Node, Ontology, Paths};
use crate::sare::{Counter, Ordered};
use std::collections::HashMap;

pub const SOURCES: [&str; 5] = ["pages", "git", "github", "linear", "watch"];

pub fn infer_type(id: &str) -> &'static str {
    if id.starts_with("commit:") {
        return "commit";
    }
    if id.starts_with("path:") {
        return "path";
    }
    if id.starts_with("gh:") && id.contains('#') {
        return "pr";
    }
    if id.starts_with("gh:") || id.starts_with("local:") {
        return "repo";
    }
    if id.starts_with("lin:project:") {
        return "milestone";
    }
    if id.starts_with("lin:cycle:") {
        return "cycle";
    }
    if id.starts_with("svc:") {
        return "service";
    }
    ""
}

pub struct Fused {
    pub nodes: Ordered<Node>,
    pub edges: Ordered<Edge>,
    pub rejected: Vec<(Edge, String)>,
}

pub fn py_list(items: &[String]) -> String {
    let inner: Vec<String> = items.iter().map(|i| format!("'{i}'")).collect();
    format!("[{}]", inner.join(", "))
}

fn key_of(e: &Edge) -> String {
    format!("{}\t{}\t{}", e.src, e.rel, e.dst)
}

pub fn fuse(paths: &Paths, ont: &Ontology) -> Fused {
    let mut nodes: Ordered<Node> = Ordered::new();
    let mut edges: Ordered<Edge> = Ordered::new();
    let mut rejected: Vec<(Edge, String)> = Vec::new();

    for row in kg::read_tsv(&paths.graph().join("services.tsv")) {
        let id = kg::get(&row, "id").to_string();
        let mut n = Node::new(&id, "service");
        n.title = kg::get(&row, "title").to_string();
        n.summary = kg::get(&row, "summary").to_string();
        nodes.insert(&id, n);
    }

    for name in SOURCES {
        let raw = kg::load_raw(paths, name);
        for n in raw.nodes {
            let id = ont.canon(&n.id);
            let mut n = n.clone();
            n.id = id.clone();
            match nodes.get_mut(&id) {
                Some(cur) => {
                    for col in kg::NODE_COLS {
                        let incoming = n.field(col);
                        if !incoming.is_empty() && cur.field(col).is_empty() {
                            cur.set(col, incoming);
                        }
                    }
                }
                None => nodes.insert(&id, n),
            }
        }
        for e in raw.edges {
            let mut e = e.clone();
            e.src = ont.canon(&e.src);
            e.dst = ont.canon(&e.dst);
            if e.conf == 0.0 {
                e.conf = ont.conf_for(&e.prov);
            }
            let key = key_of(&e);
            match edges.get(&key).cloned() {
                Some(old) => {
                    let challenger_wins = (e.conf, e.since.clone()) > (old.conf, old.since.clone());
                    let (mut win, lose) = if challenger_wins { (e, old) } else { (old, e) };
                    if lose.prov != win.prov {
                        win.evidence = format!("{} +{}", win.evidence, lose.prov).trim().to_string();
                    }
                    edges.insert(&key, win);
                }
                None => edges.insert(&key, e),
            }
        }
    }

    let endpoints: Vec<(String, String)> = edges
        .values()
        .map(|e| (e.src.clone(), e.dst.clone()))
        .collect();
    for (src, dst) in endpoints {
        for id in [src, dst] {
            if nodes.contains(&id) {
                continue;
            }
            let kind = infer_type(&id);
            if kind.is_empty() {
                continue;
            }
            let mut n = Node::new(&id, kind);
            if kind == "pr" {
                n.status = "not-open".to_string();
            }
            nodes.insert(&id, n);
        }
    }

    let mut ok: Ordered<Edge> = Ordered::new();
    for (key, e) in edges.iter() {
        let Some(rel) = ont.relations.get(&e.rel) else {
            rejected.push((e.clone(), format!("unknown relation {}", e.rel)));
            continue;
        };
        let src_type = nodes.get(&e.src).map(|n| n.kind.clone()).unwrap_or_default();
        let dst_type = nodes.get(&e.dst).map(|n| n.kind.clone()).unwrap_or_default();
        if src_type.is_empty() {
            rejected.push((e.clone(), format!("dangling src ({} is not a node)", e.src)));
            continue;
        }
        if dst_type.is_empty() {
            rejected.push((e.clone(), format!("dangling dst ({} is not a node)", e.dst)));
            continue;
        }
        if !rel.accepts_domain(&src_type) {
            rejected.push((e.clone(), format!("{} domain is {}, src is {src_type}", e.rel, py_list(&rel.domain))));
            continue;
        }
        if !rel.accepts_range(&dst_type) {
            rejected.push((e.clone(), format!("{} range is {}, dst is {dst_type}", e.rel, py_list(&rel.range))));
            continue;
        }
        if e.src == e.dst {
            rejected.push((e.clone(), "self-edge".to_string()));
            continue;
        }
        ok.insert(key, e.clone());
    }

    let mut by_src: Ordered<Vec<String>> = Ordered::new();
    for (key, e) in ok.iter() {
        if ont.relations.get(&e.rel).map(|r| r.card.as_str()) == Some("many-to-one") {
            by_src.entry_or(&format!("{}\t{}", e.src, e.rel), Vec::new()).push(key.clone());
        }
    }
    let groups: Vec<(String, Vec<String>)> =
        by_src.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    for (group, mut keys) in groups {
        if keys.len() <= 1 {
            continue;
        }
        keys.sort_by(|a, b| {
            let ea = ok.get(a).unwrap();
            let eb = ok.get(b).unwrap();
            eb.conf
                .partial_cmp(&ea.conf)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(ea.since.cmp(&eb.since))
        });
        let (src, rel) = group.split_once('\t').unwrap_or((group.as_str(), ""));
        for k in keys.iter().skip(1) {
            if let Some(dropped) = ok.remove(k) {
                rejected.push((dropped, format!("{rel} is many-to-one, {src} already has one")));
            }
        }
    }

    Fused { nodes, edges: ok, rejected }
}

/// Provenances whose edges the wiki itself asserts. Everything else mirrors
/// GitHub or git, which keep their own history.
const WIKI_PROVS: [&str; 2] = ["page", "legacy"];

/// A node is the wiki's own when a page in this repo backs it.
fn is_wiki_node(n: &Node) -> bool {
    n.path.ends_with(".md") && !n.path.starts_with('/')
}

fn node_rows<'a>(nodes: impl Iterator<Item = &'a Node>) -> Vec<Vec<String>> {
    let mut rows: Vec<Vec<String>> = nodes.map(|n| n.row()).collect();
    rows.sort_by(|a, b| a[1].cmp(&b[1]).then(a[0].cmp(&b[0])));
    rows
}

fn edge_rows<'a>(edges: impl Iterator<Item = &'a Edge>) -> Vec<Vec<String>> {
    let mut rows: Vec<Vec<String>> = edges.map(|e| e.row()).collect();
    rows.sort_by(|a, b| a[1].cmp(&b[1]).then(a[0].cmp(&b[0])).then(a[2].cmp(&b[2])));
    rows
}

/// Writes the full graph (gitignored — most of it mirrors GitHub and
/// churned thousands of diff lines per build) and the wiki's own projection,
/// which is tracked and only moves when a page does.
pub fn write(paths: &Paths, fused: &Fused) -> std::io::Result<()> {
    kg::write_tsv(&paths.nodes_tsv(), &kg::NODE_COLS, &node_rows(fused.nodes.values()))?;
    kg::write_tsv(&paths.edges_tsv(), &kg::EDGE_COLS, &edge_rows(fused.edges.values()))?;
    kg::write_tsv(
        &paths.wiki_nodes_tsv(),
        &kg::NODE_COLS,
        &node_rows(fused.nodes.values().filter(|n| is_wiki_node(n))),
    )?;
    kg::write_tsv(
        &paths.wiki_edges_tsv(),
        &kg::EDGE_COLS,
        &edge_rows(fused.edges.values().filter(|e| WIKI_PROVS.contains(&e.prov.as_str()))),
    )?;

    let mut cols: Vec<&str> = kg::EDGE_COLS.to_vec();
    cols.push("reason");
    let rows_r: Vec<Vec<String>> = fused
        .rejected
        .iter()
        .map(|(e, why)| {
            let mut row = e.row();
            row.push(why.clone());
            row
        })
        .collect();
    kg::write_tsv(&paths.raw().join("rejected.tsv"), &cols, &rows_r)
}

pub fn report(ont: &Ontology, fused: &Fused, strict: bool) -> i32 {
    let mut by_type = Counter::new();
    for n in fused.nodes.values() {
        by_type.add(&n.kind);
    }
    let mut by_rel = Counter::new();
    let mut by_prov: HashMap<String, usize> = HashMap::new();
    let mut prov_order: Vec<String> = Vec::new();
    for e in fused.edges.values() {
        by_rel.add(&e.rel);
        if !by_prov.contains_key(&e.prov) {
            prov_order.push(e.prov.clone());
        }
        *by_prov.entry(e.prov.clone()).or_insert(0) += 1;
    }

    println!(
        "nodes {}   edges {}   rejected {}",
        fused.nodes.len(),
        fused.edges.len(),
        fused.rejected.len()
    );
    println!("\nnodes by type:");
    for (t, c) in by_type.most_common() {
        println!("  {t:12} {c:5}");
    }
    println!("\nedges by relation:");
    for (r, c) in by_rel.most_common() {
        let dep = if ont.relations.get(&r).map(|x| x.deprecated).unwrap_or(false) {
            " (deprecated)"
        } else {
            ""
        };
        println!("  {r:14} {c:5}{dep}");
    }
    let provs: Vec<String> = prov_order
        .iter()
        .map(|p| format!("'{p}': {}", by_prov[p]))
        .collect();
    println!("\nedges by provenance: {{{}}}", provs.join(", "));

    let mentions = by_rel.get("MENTIONS");
    let ratio = mentions as f64 / fused.edges.len().max(1) as f64;
    let mut findings: Vec<String> = Vec::new();
    if ratio > ont.gate.max_mentions_ratio {
        findings.push(format!(
            "MENTIONS is {:.0}% of all edges (gate: {:.0}%) — {mentions} untyped links still to retype",
            ratio * 100.0,
            ont.gate.max_mentions_ratio * 100.0
        ));
    }
    if !fused.rejected.is_empty() {
        let mut reasons = Counter::new();
        for (_, why) in &fused.rejected {
            reasons.add(why.split('(').next().unwrap_or(why).trim());
        }
        let top: Vec<String> = reasons
            .most_common()
            .into_iter()
            .take(5)
            .map(|(k, v)| format!("{k} x{v}"))
            .collect();
        findings.push(format!(
            "{} rejected edges: {}",
            fused.rejected.len(),
            top.join(", ")
        ));
    }
    if findings.is_empty() {
        println!("\ngate: pass");
    } else {
        println!("\ngate findings:");
        for f in &findings {
            println!("  ! {f}");
        }
    }
    if strict && !findings.is_empty() { 1 } else { 0 }
}
