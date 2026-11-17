use crate::sare::viz::{Scene, WORLDS, shape_of, world_of};
use std::collections::BTreeMap;

const TEMPLATE: &str = include_str!("viz.html");

pub fn render(scene: &Scene) -> String {
    let nodes = serde_json::to_string(&scene.nodes).unwrap_or_else(|_| "[]".into());
    let edges = serde_json::to_string(&scene.edges).unwrap_or_else(|_| "[]".into());

    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for n in &scene.nodes {
        *counts.entry(n.kind.as_str()).or_insert(0) += 1;
    }
    let types: Vec<serde_json::Value> = counts
        .iter()
        .map(|(kind, count)| {
            serde_json::json!({
                "t": kind,
                "c": count,
                "w": world_of(kind),
                "s": shape_of(kind),
            })
        })
        .collect();
    let worlds: Vec<serde_json::Value> = WORLDS
        .iter()
        .map(|(id, label)| serde_json::json!({"id": id, "label": label}))
        .collect();

    let meta = serde_json::json!({
        "nodes": scene.nodes.len(),
        "edges": scene.edges.len(),
        "types": types,
        "worlds": worlds,
        "built": crate::clock::stamp(),
    });

    TEMPLATE
        .replace("\"__NODES__\"", &nodes)
        .replace("\"__EDGES__\"", &edges)
        .replace("\"__META__\"", &meta.to_string())
}
