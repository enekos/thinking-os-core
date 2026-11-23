use crate::ikusi::store::{Store, shape_of, weight_of, world_of};
use crate::kg::{Ontology, Paths};
use serde_json::{Value, json};
use std::sync::{Arc, OnceLock, RwLock};
use sutegi::prelude::*;

pub const DEFAULT_PORT: u16 = 4747;

/// One consistent read of the graph. The whole-graph layout is expensive
/// (~1.5s) and only changes when the TSVs do, so it is computed once per
/// snapshot and thrown away with it.
pub struct Snapshot {
    pub store: Store,
    scene: OnceLock<String>,
}

impl Snapshot {
    fn new(store: Store) -> Snapshot {
        Snapshot { store, scene: OnceLock::new() }
    }

    fn scene(&self) -> &str {
        self.scene.get_or_init(|| {
            let members: Vec<usize> = (0..self.store.nodes.len()).collect();
            scene_json(&self.store, &members, &[], 320).to_string()
        })
    }
}

pub struct Live {
    pub paths: Paths,
    pub ont: Ontology,
    snapshot: RwLock<Arc<Snapshot>>,
}

impl Live {
    pub fn new(paths: Paths, ont: Ontology) -> Live {
        let store = Store::load(&paths, &ont);
        let snapshot = Arc::new(Snapshot::new(store));
        warm(&snapshot);
        Live { paths, ont, snapshot: RwLock::new(snapshot) }
    }

    pub fn current(&self) -> Arc<Snapshot> {
        {
            let held = self.snapshot.read().unwrap();
            if !held.store.is_stale(&self.paths) {
                return held.clone();
            }
        }
        let mut held = self.snapshot.write().unwrap();
        if held.store.is_stale(&self.paths) {
            *held = Arc::new(Snapshot::new(Store::load(&self.paths, &self.ont)));
            warm(&held);
        }
        held.clone()
    }
}

/// Lay the whole graph out on a background thread. It is the one expensive
/// thing here and it is wanted the moment someone clicks "whole graph" — off
/// the request path it costs nothing, on it it is a several-second stall.
fn warm(snapshot: &Arc<Snapshot>) {
    let snapshot = snapshot.clone();
    std::thread::spawn(move || {
        snapshot.scene();
    });
}

fn ok(value: &Value) -> Response {
    Response::new(200)
        .with_header("content-type", "application/json; charset=utf-8")
        .with_header("cache-control", "no-store")
        .with_body(value.to_string().into_bytes())
}

fn fail(status: u16, message: &str) -> Response {
    Response::new(status)
        .with_header("content-type", "application/json; charset=utf-8")
        .with_body(json!({ "error": message }).to_string().into_bytes())
}

fn brief(store: &Store, i: usize) -> Value {
    let n = &store.nodes[i];
    json!({
        "id": n.id,
        "type": n.kind,
        "title": if n.title.is_empty() { n.id.clone() } else { n.title.clone() },
        "untitled": n.title.is_empty(),
        "summary": n.summary,
        "status": n.status,
        "state": n.state,
        "updated": n.updated,
        "degree": store.degree[i],
        "world": world_of(&n.kind),
        "shape": shape_of(&n.kind),
        "link": store.link_of(n),
        "local": store.local_of(n).map(|p| p.to_string_lossy().to_string()),
    })
}

fn scene_json(store: &Store, members: &[usize], hops: &[usize], iterations: usize) -> Value {
    let placed = store.layout(members, iterations);
    let nodes: Vec<Value> = members
        .iter()
        .enumerate()
        .map(|(local, &i)| {
            let n = &store.nodes[i];
            json!({
                "id": n.id,
                "t": n.kind,
                "n": if n.title.is_empty() { n.id.clone() } else { n.title.clone() },
                "s": n.summary,
                "x": (placed[local].0 * 10.0).round() / 10.0,
                "y": (placed[local].1 * 10.0).round() / 10.0,
                "d": store.degree[i],
                "h": hops.get(local).copied().unwrap_or(0),
            })
        })
        .collect();
    let edges: Vec<Value> = store
        .induced(members)
        .into_iter()
        .map(|(a, b, e)| {
            json!({ "s": a, "t": b, "r": e.rel, "c": e.conf, "p": e.prov })
        })
        .collect();
    json!({ "nodes": nodes, "edges": edges })
}

fn meta(store: &Store) -> Value {
    let types: Vec<Value> = {
        let mut rows: Vec<(&str, usize)> = store.counts_by_type().into_iter().collect();
        rows.sort_by(|a, b| weight_of(b.0).cmp(&weight_of(a.0)).then(b.1.cmp(&a.1)));
        rows.into_iter()
            .map(|(kind, count)| {
                json!({ "t": kind, "c": count, "w": world_of(kind), "s": shape_of(kind) })
            })
            .collect()
    };
    let relations: Vec<Value> = {
        let mut rows: Vec<(&str, usize)> = store.counts_by_rel().into_iter().collect();
        rows.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
        rows.into_iter()
            .map(|(rel, count)| {
                json!({ "r": rel, "c": count, "deprecated": store.deprecated.contains(rel) })
            })
            .collect()
    };
    json!({
        "nodes": store.nodes.len(),
        "edges": store.edges.len(),
        "dangling": store.dangling,
        "loaded_at": store.loaded_at,
        "root": store.root.to_string_lossy(),
        "types": types,
        "relations": relations,
        "worlds": crate::sare::viz::WORLDS.iter().map(|(id, label)| json!({"id": id, "label": label})).collect::<Vec<_>>(),
    })
}

/// Everything that touches this node, grouped by relation and split by
/// direction. Direction is part of an edge's meaning here — a decision
/// DECIDES a project and the project never links back — so collapsing the
/// two halves would lose what the ontology is for.
fn detail(store: &Store, i: usize) -> Value {
    let n = &store.nodes[i];

    let group = |edge_ids: &[usize], outbound: bool| -> Vec<Value> {
        let mut by_rel: std::collections::BTreeMap<&str, Vec<Value>> = Default::default();
        for &e in edge_ids {
            let edge = &store.edges[e];
            let other = if outbound { edge.dst } else { edge.src };
            let mut item = brief(store, other);
            item["since"] = json!(edge.since);
            item["prov"] = json!(edge.prov);
            item["conf"] = json!(edge.conf);
            item["evidence"] = json!(edge.evidence);
            by_rel.entry(edge.rel.as_str()).or_default().push(item);
        }
        let mut groups: Vec<Value> = by_rel
            .into_iter()
            .map(|(rel, mut items)| {
                items.sort_by(|a, b| {
                    let key = |v: &Value| {
                        (
                            -weight_of(v["type"].as_str().unwrap_or("")),
                            -(v["degree"].as_i64().unwrap_or(0) as i32),
                            v["title"].as_str().unwrap_or("").to_lowercase(),
                        )
                    };
                    key(a).cmp(&key(b))
                });
                json!({
                    "rel": rel,
                    "dir": if outbound { "out" } else { "in" },
                    "deprecated": store.deprecated.contains(rel),
                    "count": items.len(),
                    "items": items,
                })
            })
            .collect();
        // MENTIONS is retyping debt, not a finding: it sinks below every real
        // verb however many of them there are.
        groups.sort_by(|a, b| {
            a["deprecated"].as_bool().cmp(&b["deprecated"].as_bool())
                .then_with(|| b["count"].as_u64().cmp(&a["count"].as_u64()))
                .then_with(|| a["rel"].as_str().unwrap_or("").cmp(b["rel"].as_str().unwrap_or("")))
        });
        groups
    };

    let mut node = brief(store, i);
    node["path"] = json!(n.path);
    node["url"] = json!(n.url);

    json!({
        "node": node,
        "body": store.body_of(n),
        "out": group(&store.out[i], true),
        "in": group(&store.inb[i], false),
        "counts": { "out": store.out[i].len(), "in": store.inb[i].len() },
    })
}

fn param(c: &Ctx, key: &str) -> Option<String> {
    c.query().get(key).map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

fn number(c: &Ctx, key: &str, fallback: usize, cap: usize) -> usize {
    param(c, key).and_then(|v| v.parse().ok()).unwrap_or(fallback).min(cap)
}

pub fn app(live: Live) -> App {
    App::new("ikusi")
        .state(live)
        .get("/api/meta", "Graph totals, node types and relation counts.", |c: &Ctx| {
            ok(&meta(&c.state::<Live>().current().store))
        })
        .get("/api/search", "Rank nodes against a query.", |c: &Ctx| {
            let snap = c.state::<Live>().current();
            let query = param(c, "q").unwrap_or_default();
            let kinds: Vec<String> = param(c, "type")
                .map(|v| v.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect())
                .unwrap_or_default();
            let limit = number(c, "limit", 40, 300);
            let total_nodes = snap.store.nodes.len();
            let offset = number(c, "offset", 0, total_nodes);
            let ranked = snap.store.search(&query, &kinds, total_nodes);
            let total = ranked.len();
            let hits: Vec<Value> = ranked
                .into_iter()
                .skip(offset)
                .take(limit)
                .map(|(i, score)| {
                    let mut item = brief(&snap.store, i);
                    item["score"] = json!(score);
                    item
                })
                .collect();
            ok(&json!({ "q": query, "count": hits.len(), "total": total, "hits": hits }))
        })
        .get("/api/ids", "Every node id — lets the client tell a live [[link]] from a dead one.", |c: &Ctx| {
            let snap = c.state::<Live>().current();
            let ids: Vec<&str> = snap.store.nodes.iter().map(|n| n.id.as_str()).collect();
            ok(&json!({ "ids": ids }))
        })
        .get("/api/node", "One node with every edge that touches it.", |c: &Ctx| {
            let snap = c.state::<Live>().current();
            let Some(id) = param(c, "id") else {
                return fail(400, "id is required");
            };
            match snap.store.idx(&id) {
                Some(i) => ok(&detail(&snap.store, i)),
                None => fail(404, &format!("no node `{id}`")),
            }
        })
        .get("/api/subgraph", "The neighbourhood of one node, laid out.", |c: &Ctx| {
            let snap = c.state::<Live>().current();
            let Some(id) = param(c, "id") else {
                return fail(400, "id is required");
            };
            let Some(focus) = snap.store.idx(&id) else {
                return fail(404, &format!("no node `{id}`"));
            };
            let depth = number(c, "depth", 2, 6).max(1);
            let limit = number(c, "limit", 200, 1200);
            let hub = number(c, "hub", 25, 5000).max(1);
            let (reached, held) = snap.store.neighbourhood(focus, depth, limit, hub);
            let members: Vec<usize> = reached.iter().map(|(i, _)| *i).collect();
            let hops: Vec<usize> = reached.iter().map(|(_, d)| *d).collect();
            let mut scene = scene_json(&snap.store, &members, &hops, 260);
            scene["focus"] = json!(id);
            scene["depth"] = json!(depth);
            scene["held"] = json!(held);
            scene["truncated"] = json!(members.len() >= limit);
            ok(&scene)
        })
        .get("/api/scene", "The whole graph, laid out once and cached.", |c: &Ctx| {
            Response::new(200)
                .with_header("content-type", "application/json; charset=utf-8")
                .with_body(c.state::<Live>().current().scene().as_bytes().to_vec())
        })
        .get("/api/status", "My latest open PRs and Linear issues, from the poller's snapshot.", |c: &Ctx| {
            let live = c.state::<Live>();
            match crate::ikusi::status::load(&live.paths.root) {
                Some(snap) => ok(&crate::ikusi::status::board(&snap, number(c, "limit", 12, 100).max(1))),
                None => fail(404, "zaindu has not polled yet — run `zaindu poll`"),
            }
        })
        .post("/api/open", "Open a node's file locally (editor, Finder).", |c: &Ctx| {
            let snap = c.state::<Live>().current();
            let Some(id) = param(c, "id") else {
                return fail(400, "id is required");
            };
            let Some(i) = snap.store.idx(&id) else {
                return fail(404, &format!("no node `{id}`"));
            };
            let Some(path) = snap.store.local_of(&snap.store.nodes[i]) else {
                return fail(404, "this node has no local file");
            };
            let how = param(c, "how").unwrap_or_else(|| "edit".into());
            match spawn_open(&path, &how) {
                Ok(()) => ok(&json!({ "opened": path.to_string_lossy(), "how": how })),
                Err(e) => fail(500, &e),
            }
        })
        .get("/licenses.txt", "Licenses for the bundled viewer assets.", |_: &Ctx| {
            Response::new(200)
                .with_header("content-type", "text/plain; charset=utf-8")
                .with_body(include_bytes!("web/licenses.md").to_vec())
        })
        .get("/app.css", "Stylesheet.", |_: &Ctx| {
            Response::new(200)
                .with_header("content-type", "text/css; charset=utf-8")
                .with_body(include_str!("web/app.css").as_bytes().to_vec())
        })
        .get("/app.js", "Client.", |_: &Ctx| {
            Response::new(200)
                .with_header("content-type", "application/javascript; charset=utf-8")
                .with_body(include_str!("web/app.js").as_bytes().to_vec())
        })
        .get("/", "The viewer.", |_: &Ctx| html(200, include_str!("web/index.html")))
        .get("/*rest", "The viewer — client-side routes fall through to it.", |_: &Ctx| {
            html(200, include_str!("web/index.html"))
        })
}

fn spawn_open(path: &std::path::Path, how: &str) -> Result<(), String> {
    let editor = std::env::var("IZPI_EDITOR")
        .or_else(|_| std::env::var("VISUAL"))
        .or_else(|_| std::env::var("EDITOR"))
        .unwrap_or_else(|_| if cfg!(target_os = "macos") { "zed" } else { "xdg-open" }.to_string());
    let mut cmd = match how {
        "reveal" if cfg!(target_os = "macos") => {
            let mut c = std::process::Command::new("open");
            c.arg("-R").arg(path);
            c
        }
        "reveal" => {
            let dir = if path.is_dir() { path } else { path.parent().unwrap_or(path) };
            let mut c = std::process::Command::new(crate::ikusi::opener());
            c.arg(dir);
            c
        }
        "terminal" if cfg!(target_os = "macos") => {
            let dir = if path.is_dir() { path } else { path.parent().unwrap_or(path) };
            let mut c = std::process::Command::new("open");
            c.arg("-a").arg("Terminal").arg(dir);
            c
        }
        "terminal" => {
            let dir = if path.is_dir() { path } else { path.parent().unwrap_or(path) };
            let term = std::env::var("TERMINAL").unwrap_or_else(|_| "x-terminal-emulator".to_string());
            let mut c = std::process::Command::new(term);
            c.current_dir(dir);
            c
        }
        _ => {
            let mut c = std::process::Command::new(&editor);
            c.arg(path);
            c
        }
    };
    cmd.stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("cannot run {editor}: {e}"))
}
