use crate::kg::{self, Paths, Row};
use crate::sare::serve::Graph;
use serde::Serialize;
use std::collections::HashMap;

pub const WORLDS: [(&str, &str); 2] = [("wiki", "this wiki"), ("code", "code & runtime")];

pub fn world_of(kind: &str) -> &'static str {
    match kind {
        "project" | "task" | "attempt" | "test" | "concept" | "decision" | "person" | "source" | "briefing" => "wiki",
        _ => "code",
    }
}

pub fn shape_of(kind: &str) -> usize {
    match kind {
        "project" | "repo" => 0,
        "concept" | "service" => 1,
        "decision" | "pr" => 2,
        "person" => 3,
        "source" => 4,
        _ => 5,
    }
}

#[derive(Serialize)]
pub struct VizNode {
    pub id: String,
    #[serde(rename = "t")]
    pub kind: String,
    #[serde(rename = "n")]
    pub title: String,
    #[serde(rename = "s")]
    pub summary: String,
    #[serde(rename = "p")]
    pub path: String,
    pub x: f32,
    pub y: f32,
    #[serde(rename = "d")]
    pub degree: usize,
}

#[derive(Serialize)]
pub struct VizEdge {
    #[serde(rename = "s")]
    pub src: usize,
    #[serde(rename = "t")]
    pub dst: usize,
    #[serde(rename = "r")]
    pub rel: String,
    #[serde(rename = "c")]
    pub conf: f32,
}

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        ((self.0 >> 11) as f32) / ((1u64 << 53) as f32)
    }
}

const FRAME: f32 = 1600.0;



fn components(count: usize, links: &[(usize, usize)]) -> Vec<Vec<usize>> {
    let mut parent: Vec<usize> = (0..count).collect();
    fn find(parent: &mut Vec<usize>, mut x: usize) -> usize {
        while parent[x] != x {
            parent[x] = parent[parent[x]];
            x = parent[x];
        }
        x
    }
    for (a, b) in links {
        let (ra, rb) = (find(&mut parent, *a), find(&mut parent, *b));
        if ra != rb {
            parent[ra] = rb;
        }
    }
    let mut groups: HashMap<usize, Vec<usize>> = HashMap::new();
    for i in 0..count {
        let root = find(&mut parent, i);
        groups.entry(root).or_default().push(i);
    }
    let mut out: Vec<Vec<usize>> = groups.into_values().collect();
    out.sort_by(|a, b| b.len().cmp(&a.len()).then(a[0].cmp(&b[0])));
    out
}

fn spring(members: &[usize], links: &[(usize, usize)], iterations: usize, seed: u64) -> Vec<(f32, f32)> {
    let count = members.len();
    let local: HashMap<usize, usize> = members.iter().enumerate().map(|(i, m)| (*m, i)).collect();
    let inner: Vec<(usize, usize)> = links
        .iter()
        .filter_map(|(a, b)| Some((*local.get(a)?, *local.get(b)?)))
        .collect();

    let frame = FRAME * (count as f32 / 400.0).sqrt().clamp(0.25, 3.0);
    let mut rng = Rng(seed);
    let mut pos: Vec<(f32, f32)> = (0..count)
        .map(|_| (rng.next() * frame, rng.next() * frame))
        .collect();
    if count == 1 {
        return vec![(0.0, 0.0)];
    }
    let k = (frame * frame / count as f32).sqrt();
    let mut temp = frame / 8.0;
    let cool = temp / (iterations as f32 + 1.0);

    for _ in 0..iterations {
        let mut disp = vec![(0.0f32, 0.0f32); count];
        for i in 0..count {
            for j in (i + 1)..count {
                let dx = pos[i].0 - pos[j].0;
                let dy = pos[i].1 - pos[j].1;
                let d = (dx * dx + dy * dy).max(0.01).sqrt();
                let force = k * k / d;
                let (ux, uy) = (dx / d * force, dy / d * force);
                disp[i].0 += ux;
                disp[i].1 += uy;
                disp[j].0 -= ux;
                disp[j].1 -= uy;
            }
        }
        for (u, v) in &inner {
            let dx = pos[*u].0 - pos[*v].0;
            let dy = pos[*u].1 - pos[*v].1;
            let d = (dx * dx + dy * dy).max(0.01).sqrt();
            let force = d * d / k;
            let (ux, uy) = (dx / d * force, dy / d * force);
            disp[*u].0 -= ux;
            disp[*u].1 -= uy;
            disp[*v].0 += ux;
            disp[*v].1 += uy;
        }
        let centre = frame / 2.0;
        for i in 0..count {
            disp[i].0 += (centre - pos[i].0) * 0.02;
            disp[i].1 += (centre - pos[i].1) * 0.02;
            let d = (disp[i].0 * disp[i].0 + disp[i].1 * disp[i].1).max(0.01).sqrt();
            let step = d.min(temp);
            pos[i].0 += disp[i].0 / d * step;
            pos[i].1 += disp[i].1 / d * step;
        }
        temp -= cool;
    }
    recentre(&mut pos);
    pos
}

fn recentre(pos: &mut [(f32, f32)]) {
    let n = pos.len() as f32;
    let cx = pos.iter().map(|p| p.0).sum::<f32>() / n;
    let cy = pos.iter().map(|p| p.1).sum::<f32>() / n;
    for p in pos.iter_mut() {
        p.0 -= cx;
        p.1 -= cy;
    }
}

fn extent(pos: &[(f32, f32)]) -> f32 {
    pos.iter()
        .map(|p| (p.0 * p.0 + p.1 * p.1).sqrt())
        .fold(0.0f32, f32::max)
        .max(6.0)
}

/// One force-directed layout per connected component, then the small ones are
/// packed in rings around the giant one. Left to a single global pass, the ~60
/// nodes that connect to nothing drift into a huge halo and squeeze the core
/// into an unreadable dot.
pub fn layout(count: usize, links: &[(usize, usize)], iterations: usize) -> Vec<(f32, f32)> {
    if count == 0 {
        return Vec::new();
    }
    let groups = components(count, links);
    let mut pos = vec![(0.0f32, 0.0f32); count];

    let core = &groups[0];
    let core_pos = spring(core, links, iterations, 0x5EED_1234_ABCD_9876);
    for (i, member) in core.iter().enumerate() {
        pos[*member] = core_pos[i];
    }
    // The small components are packed into a tidy grid beside the core rather
    // than a ring around it: a ring has to clear the core's radius, which leaves
    // the interesting middle a speck inside a mostly empty circle.
    let small: Vec<(usize, &Vec<usize>)> = groups.iter().enumerate().skip(1).collect();
    let laid: Vec<Vec<(f32, f32)>> = small
        .iter()
        .map(|(gi, group)| spring(group, links, iterations.min(160), 0xC0FFEE ^ *gi as u64))
        .collect();

    if !small.is_empty() {
        let core_r = extent(&core_pos);
        let cell = laid.iter().map(|l| extent(l)).fold(8.0f32, f32::max) * 2.0 + 26.0;
        let cols = ((small.len() as f32).sqrt().ceil() as usize).max(1);
        let rows = small.len().div_ceil(cols);
        let left = core_r + cell;
        let top = -(rows as f32 * cell) / 2.0 + cell / 2.0;
        for (i, (_, group)) in small.iter().enumerate() {
            let (col, row) = (i % cols, i / cols);
            let cx = left + col as f32 * cell;
            let cy = top + row as f32 * cell;
            for (j, member) in group.iter().enumerate() {
                pos[*member] = (cx + laid[i][j].0, cy + laid[i][j].1);
            }
        }
    }

    let shift = extent(&pos) + 40.0;
    for p in pos.iter_mut() {
        p.0 += shift;
        p.1 += shift;
    }
    pos
}

pub struct Scene {
    pub nodes: Vec<VizNode>,
    pub edges: Vec<VizEdge>,
}

pub fn build(graph: &Graph, iterations: usize) -> Scene {
    let rows: Vec<&Row> = graph.nodes.values().collect();
    let index: HashMap<&str, usize> = rows
        .iter()
        .enumerate()
        .map(|(i, r)| (kg::get(r, "id"), i))
        .collect();

    let mut links: Vec<(usize, usize)> = Vec::new();
    let mut degree = vec![0usize; rows.len()];
    let mut edges: Vec<VizEdge> = Vec::new();
    for e in &graph.edges {
        let (Some(&s), Some(&t)) = (index.get(kg::get(e, "src")), index.get(kg::get(e, "dst"))) else {
            continue;
        };
        links.push((s, t));
        degree[s] += 1;
        degree[t] += 1;
        edges.push(VizEdge {
            src: s,
            dst: t,
            rel: kg::get(e, "rel").to_string(),
            conf: kg::get(e, "conf").parse().unwrap_or(1.0),
        });
    }

    let placed = layout(rows.len(), &links, iterations);
    let nodes = rows
        .iter()
        .enumerate()
        .map(|(i, r)| VizNode {
            id: kg::get(r, "id").to_string(),
            kind: kg::get(r, "type").to_string(),
            title: kg::get(r, "title").to_string(),
            summary: kg::get(r, "summary").to_string(),
            path: kg::get(r, "path").to_string(),
            x: (placed[i].0 * 10.0).round() / 10.0,
            y: (placed[i].1 * 10.0).round() / 10.0,
            degree: degree[i],
        })
        .collect();
    Scene { nodes, edges }
}

pub fn render_dot(scene: &Scene) -> String {
    let mut out = String::from("digraph thinking_os {\n  overlap=false;\n  splines=true;\n  node [shape=circle, style=filled, fontsize=9];\n");
    for n in &scene.nodes {
        let colour = match world_of(&n.kind) {
            "wiki" => "#2a78d6",
            _ => "#1baf7a",
        };
        out.push_str(&format!(
            "  \"{}\" [fillcolor=\"{colour}\", tooltip=\"{}\"];\n",
            n.id.replace('"', "'"),
            n.kind
        ));
    }
    for e in &scene.edges {
        out.push_str(&format!(
            "  \"{}\" -> \"{}\" [label=\"{}\"];\n",
            scene.nodes[e.src].id.replace('"', "'"),
            scene.nodes[e.dst].id.replace('"', "'"),
            e.rel
        ));
    }
    out.push_str("}\n");
    out
}

pub fn run(paths: &Paths, out: Option<&str>, dot: bool, iterations: usize) -> i32 {
    let graph = Graph::load(paths);
    if graph.nodes.is_empty() {
        eprintln!("sare: no graph yet — run `sare build` first");
        return 1;
    }
    let scene = build(&graph, iterations);
    let default = if dot { "graph/views/graph.dot" } else { "graph/views/graph.html" };
    let target = paths.root.join(out.unwrap_or(default));
    let body = if dot { render_dot(&scene) } else { super::viz_html::render(&scene) };
    match crate::kg::write_atomic(&target, body.as_bytes()) {
        Ok(()) => {
            let mut kinds: Vec<(String, usize)> = {
                let mut counter: HashMap<String, usize> = HashMap::new();
                for n in &scene.nodes {
                    *counter.entry(n.kind.clone()).or_insert(0) += 1;
                }
                counter.into_iter().collect()
            };
            kinds.sort_by(|a, b| b.1.cmp(&a.1));
            println!(
                "wrote {} — {} nodes, {} edges, {} types, {} KB",
                paths.rel(&target),
                scene.nodes.len(),
                scene.edges.len(),
                kinds.len(),
                body.len() / 1024
            );
            println!("open it with: open {}", paths.rel(&target));
            0
        }
        Err(e) => {
            eprintln!("sare: cannot write {}: {e}", target.display());
            1
        }
    }
}

pub fn placement_is_spread(scene: &Scene) -> bool {
    let xs: Vec<f32> = scene.nodes.iter().map(|n| n.x).collect();
    let ys: Vec<f32> = scene.nodes.iter().map(|n| n.y).collect();
    let span = |v: &[f32]| {
        let lo = v.iter().cloned().fold(f32::MAX, f32::min);
        let hi = v.iter().cloned().fold(f32::MIN, f32::max);
        hi - lo
    };
    scene.nodes.len() < 2 || (span(&xs) > 10.0 && span(&ys) > 10.0)
}
