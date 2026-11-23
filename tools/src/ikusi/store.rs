use crate::kg::{self, Ontology, Paths};
use crate::sare::viz;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::time::SystemTime;


pub struct NodeRec {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub summary: String,
    pub status: String,
    pub state: String,
    pub updated: String,
    pub path: String,
    pub url: String,
    id_lc: String,
    title_lc: String,
    summary_lc: String,
}

impl NodeRec {
    pub(super) fn sort_title(&self) -> &str {
        if self.title.is_empty() { &self.id_lc } else { &self.title_lc }
    }

    pub fn new(id: &str, kind: &str, title: &str, summary: &str) -> NodeRec {
        NodeRec {
            id_lc: id.to_lowercase(),
            title_lc: title.to_lowercase(),
            summary_lc: summary.to_lowercase(),
            id: id.into(),
            kind: kind.into(),
            title: title.into(),
            summary: summary.into(),
            status: String::new(),
            state: String::new(),
            updated: String::new(),
            path: String::new(),
            url: String::new(),
        }
    }
}

pub struct EdgeRec {
    pub src: usize,
    pub dst: usize,
    pub rel: String,
    pub since: String,
    pub prov: String,
    pub conf: f32,
    pub evidence: String,
}

pub struct Store {
    pub root: PathBuf,
    pub nodes: Vec<NodeRec>,
    pub by_id: HashMap<String, usize>,
    pub edges: Vec<EdgeRec>,
    pub out: Vec<Vec<usize>>,
    pub inb: Vec<Vec<usize>>,
    pub degree: Vec<usize>,
    pub deprecated: HashSet<String>,
    pub loaded_at: String,
    pub dangling: usize,
    stamp: (u128, u128),
}

fn mtime(path: &std::path::Path) -> u128 {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}

/// Type weight — what a human means by "important" when two nodes match a
/// query equally well. A project beats the PR that touched it.
pub fn weight_of(kind: &str) -> i32 {
    match kind {
        "project" => 30,
        "concept" => 18,
        "task" => 16,
        "decision" => 14,
        "service" => 12,
        "milestone" => 10,
        "repo" => 8,
        "issue" => 6,
        "person" => 4,
        "pr" => 2,
        "test" => 2,
        "attempt" => 1,
        "cycle" => 1,
        _ => 0,
    }
}

impl Store {
    pub fn load(paths: &Paths, ont: &Ontology) -> Store {
        let stamp = (mtime(&paths.nodes_tsv()), mtime(&paths.edges_tsv()));
        let mut nodes: Vec<NodeRec> = Vec::new();
        let mut by_id: HashMap<String, usize> = HashMap::new();
        for row in kg::read_tsv(&paths.nodes_tsv()) {
            let get = |c: &str| kg::get(&row, c).to_string();
            let id = get("id");
            if id.is_empty() || by_id.contains_key(&id) {
                continue;
            }
            let title = get("title");
            let summary = get("summary");
            by_id.insert(id.clone(), nodes.len());
            nodes.push(NodeRec {
                id_lc: id.to_lowercase(),
                title_lc: title.to_lowercase(),
                summary_lc: summary.to_lowercase(),
                id,
                kind: get("type"),
                title,
                summary,
                status: get("status"),
                state: get("state"),
                updated: get("updated"),
                path: get("path"),
                url: get("url"),
            });
        }

        let mut edges: Vec<EdgeRec> = Vec::new();
        let mut dangling = 0usize;
        for row in kg::read_tsv(&paths.edges_tsv()) {
            let get = |c: &str| kg::get(&row, c).to_string();
            let (Some(&src), Some(&dst)) = (by_id.get(&get("src")), by_id.get(&get("dst"))) else {
                dangling += 1;
                continue;
            };
            edges.push(EdgeRec {
                src,
                dst,
                rel: get("rel"),
                since: get("since"),
                prov: get("prov"),
                conf: get("conf").parse().unwrap_or(1.0),
                evidence: get("evidence"),
            });
        }

        let deprecated = ont
            .relations
            .iter()
            .filter(|(_, r)| r.deprecated)
            .map(|(name, _)| name.clone())
            .collect();

        Store {
            dangling,
            deprecated,
            stamp,
            ..Store::from_rows(paths.root.clone(), nodes, edges)
        }
    }

    pub fn from_rows(root: PathBuf, nodes: Vec<NodeRec>, edges: Vec<EdgeRec>) -> Store {
        let by_id = nodes.iter().enumerate().map(|(i, n)| (n.id.clone(), i)).collect();
        let mut out = vec![Vec::new(); nodes.len()];
        let mut inb = vec![Vec::new(); nodes.len()];
        let mut degree = vec![0usize; nodes.len()];
        for (i, e) in edges.iter().enumerate() {
            out[e.src].push(i);
            inb[e.dst].push(i);
            degree[e.src] += 1;
            degree[e.dst] += 1;
        }
        Store {
            root,
            nodes,
            by_id,
            edges,
            out,
            inb,
            degree,
            deprecated: HashSet::new(),
            loaded_at: crate::clock::stamp(),
            dangling: 0,
            stamp: (0, 0),
        }
    }

    pub(super) fn revision(&self) -> String {
        format!("{:x}-{:x}", self.stamp.0, self.stamp.1)
    }

    /// The graph is rebuilt by a different process (`sare build`, zaindu).
    /// Two `stat` calls per request is the whole price of never serving a
    /// stale page.
    pub fn is_stale(&self, paths: &Paths) -> bool {
        self.stamp != (mtime(&paths.nodes_tsv()), mtime(&paths.edges_tsv()))
    }

    pub fn idx(&self, id: &str) -> Option<usize> {
        self.by_id.get(id).copied()
    }

    pub fn counts_by_type(&self) -> BTreeMap<&str, usize> {
        let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
        for n in &self.nodes {
            *counts.entry(n.kind.as_str()).or_insert(0) += 1;
        }
        counts
    }

    pub fn counts_by_rel(&self) -> BTreeMap<&str, usize> {
        let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
        for e in &self.edges {
            *counts.entry(e.rel.as_str()).or_insert(0) += 1;
        }
        counts
    }

    /// Where this node lives outside the wiki. Half the nodes already carry a
    /// `url`; the rest are reconstructible from the id, which is why ids are
    /// shaped `gh:owner/repo#n` and `svc:host` in the first place.
    pub fn link_of(&self, n: &NodeRec) -> Option<String> {
        if !n.url.is_empty() {
            return Some(n.url.clone());
        }
        if let Some(rest) = n.id.strip_prefix("gh:") {
            return Some(match rest.split_once('#') {
                Some((repo, number)) => format!("https://github.com/{repo}/pull/{number}"),
                None => format!("https://github.com/{rest}"),
            });
        }
        if let Some(host) = n.id.strip_prefix("svc:") {
            return Some(format!("https://{host}"));
        }
        None
    }

    /// The file to open in an editor: a wiki page is relative to the root, a
    /// repo's `path` is already absolute.
    pub fn local_of(&self, n: &NodeRec) -> Option<PathBuf> {
        if n.path.is_empty() {
            return None;
        }
        let p = PathBuf::from(&n.path);
        let full = if p.is_absolute() { p } else { self.root.join(p) };
        full.exists().then_some(full)
    }

    /// The markdown body of a wiki page, frontmatter stripped — the sidebar
    /// shows the page itself, not a summary of it.
    pub fn body_of(&self, n: &NodeRec) -> Option<String> {
        let path = self.local_of(n)?;
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            return None;
        }
        let front = kg::page::frontmatter(&path);
        let body = front.body.trim().to_string();
        (!body.is_empty()).then_some(body)
    }

    pub fn search(&self, query: &str, kinds: &[String], limit: usize) -> Vec<(usize, i32)> {
        let terms: Vec<String> = query
            .to_lowercase()
            .split_whitespace()
            .map(|t| t.to_string())
            .filter(|t| !t.is_empty())
            .collect();

        let mut hits: Vec<(usize, i32)> = Vec::new();
        for (i, n) in self.nodes.iter().enumerate() {
            if !kinds.is_empty() && !kinds.iter().any(|k| k == &n.kind) {
                continue;
            }
            let mut score = 0i32;
            if !terms.is_empty() {
                let mut matched = true;
                for term in &terms {
                    let s = self.term_score(n, term);
                    if s == 0 {
                        matched = false;
                        break;
                    }
                    score += s;
                }
                if !matched {
                    continue;
                }
            }
            score += weight_of(&n.kind) * 6;
            score += (self.degree[i] as i32).min(50);
            if n.title.is_empty() {
                score -= 150;
            }
            hits.push((i, score));
        }

        hits.sort_by(|a, b| {
            b.1.cmp(&a.1)
                .then_with(|| self.degree[b.0].cmp(&self.degree[a.0]))
                .then_with(|| self.nodes[a.0].id.cmp(&self.nodes[b.0].id))
        });
        hits.truncate(limit);
        hits
    }

    /// One term against one node. The shape of the score matters more than the
    /// numbers: an id is a stronger signal than a title, a match at a word
    /// boundary is stronger than one in the middle, and a subsequence is the
    /// last resort so a typo still finds something.
    fn term_score(&self, n: &NodeRec, term: &str) -> i32 {
        if n.id_lc == term {
            return 1200;
        }
        let mut best = 0i32;

        if term.bytes().all(|b| b.is_ascii_digit()) && n.id_lc.ends_with(&format!("#{term}")) {
            best = best.max(940);
        }
        if let Some(p) = n.id_lc.find(term) {
            best = best.max(if p == 0 {
                820
            } else if !n.id_lc.as_bytes()[p - 1].is_ascii_alphanumeric() {
                760
            } else {
                600
            });
        }
        if n.title_lc == term {
            best = best.max(800);
        } else if let Some(p) = n.title_lc.find(term) {
            best = best.max(if p == 0 {
                700
            } else if !n.title_lc.as_bytes()[p - 1].is_ascii_alphanumeric() {
                640
            } else {
                510
            });
        }
        if n.summary_lc.contains(term) {
            best = best.max(380);
        }
        if n.kind == term {
            best = best.max(340);
        }
        if n.status.eq_ignore_ascii_case(term) || n.state.eq_ignore_ascii_case(term) {
            best = best.max(320);
        }
        if best == 0 && subsequence(term, &n.id_lc) {
            best = 200;
        }
        if best == 0 && subsequence(term, &n.title_lc) {
            best = 180;
        }
        if best >= 600 {
            best -= (n.id.len() as i32).min(80) / 4;
        }
        best
    }

    /// Breadth-first out to `depth`, then trimmed to `limit` by keeping the
    /// nodes nearest the focus and, at equal distance, the ones that carry the
    /// most structure. A subgraph that blows past the limit is a hairball
    /// again, which is the thing this viewer exists to stop being.
    ///
    /// Traversal stops AT a hub instead of going through it. Two hops from a PR
    /// reach its author, and the author has 433 other PRs — expanded, the
    /// picture is about the author and the PR is a speck in the corner. The hub
    /// is still drawn, because "who wrote this" is the answer; what is dropped
    /// is everything else that happens to share it. Returns the reached set and
    /// how many nodes were held back this way.
    pub fn neighbourhood(
        &self,
        focus: usize,
        depth: usize,
        limit: usize,
        hub: usize,
    ) -> (Vec<(usize, usize)>, usize) {
        let mut depth_of: HashMap<usize, usize> = HashMap::from([(focus, 0)]);
        let mut frontier = vec![focus];
        let mut held = 0usize;
        for d in 0..depth {
            let mut next = Vec::new();
            for i in frontier {
                for &e in self.out[i].iter().chain(self.inb[i].iter()) {
                    let edge = &self.edges[e];
                    let other = if edge.src == i { edge.dst } else { edge.src };
                    if depth_of.contains_key(&other) {
                        continue;
                    }
                    depth_of.insert(other, d + 1);
                    if self.degree[other] <= hub {
                        next.push(other);
                    } else {
                        held += 1;
                    }
                }
            }
            frontier = next;
        }
        let mut reached: Vec<(usize, usize)> = depth_of.into_iter().collect();
        reached.sort_by(|a, b| {
            a.1.cmp(&b.1)
                .then_with(|| {
                    let ka = weight_of(&self.nodes[b.0].kind) + self.degree[b.0] as i32;
                    let kb = weight_of(&self.nodes[a.0].kind) + self.degree[a.0] as i32;
                    ka.cmp(&kb)
                })
                .then_with(|| self.nodes[a.0].id.cmp(&self.nodes[b.0].id))
        });
        reached.truncate(limit.max(1));
        (reached, held)
    }

    pub fn layout(&self, members: &[usize], iterations: usize) -> Vec<(f32, f32)> {
        let links: Vec<_> = self.induced(members).iter().map(|&(a, b, _)| (a, b)).collect();
        super::layout::place(members.len(), &links, iterations).positions
    }

    /// Visit only outgoing edges of members. Iterating by source emits every
    /// parallel edge and self-loop exactly once, including for small neighborhoods.
    pub fn induced(&self, members: &[usize]) -> Vec<(usize, usize, &EdgeRec)> {
        let local: HashMap<usize, usize> =
            members.iter().enumerate().map(|(i, m)| (*m, i)).collect();
        let mut result = Vec::new();
        for (a, &src) in members.iter().enumerate() {
            for &index in &self.out[src] {
                let e = &self.edges[index];
                if let Some(&b) = local.get(&e.dst) {
                    result.push((a, b, e));
                }
            }
        }
        result
    }
}

fn subsequence(needle: &str, hay: &str) -> bool {
    if needle.len() < 2 {
        return false;
    }
    let mut chars = needle.chars();
    let mut want = match chars.next() {
        Some(c) => c,
        None => return true,
    };
    for c in hay.chars() {
        if c == want {
            match chars.next() {
                Some(next) => want = next,
                None => return true,
            }
        }
    }
    false
}

pub fn world_of(kind: &str) -> &'static str {
    viz::world_of(kind)
}

pub fn shape_of(kind: &str) -> usize {
    viz::shape_of(kind)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edge(src: usize, dst: usize, rel: &str) -> EdgeRec {
        EdgeRec {
            src,
            dst,
            rel: rel.into(),
            since: String::new(),
            prov: "page".into(),
            conf: 1.0,
            evidence: String::new(),
        }
    }

    fn fixture() -> Store {
        let mut nodes = vec![
            NodeRec::new("sutegi", "project", "sutegi", "zero-dep web framework"),
            NodeRec::new("gh:join-com/data#7280", "pr", "feat: unify the prompts", ""),
            NodeRec::new("adopt-sutegi", "decision", "Save changes accepts violating content", ""),
            NodeRec::new("eneko-sarasola", "person", "Eneko Sarasola", ""),
            NodeRec::new("svc:bai.hirusta.io", "service", "bai", ""),
            NodeRec::new("llm-wiki", "concept", "LLM wiki", ""),
            NodeRec::new("gh:enekos/sutegi", "repo", "enekos/sutegi", ""),
        ];
        // the person is a hub: every PR below hangs off them
        let mut edges = vec![edge(1, 2, "DEPENDS_ON"), edge(1, 3, "AUTHORED_BY"), edge(0, 6, "LIVES_IN")];
        for n in 0..40 {
            nodes.push(NodeRec::new(&format!("gh:join-com/data#{}", 8000 + n), "pr", "noise", ""));
            edges.push(edge(7 + n, 3, "AUTHORED_BY"));
        }
        Store::from_rows(PathBuf::from("/tmp"), nodes, edges)
    }

    fn top(store: &Store, query: &str) -> String {
        store.search(query, &[], 5).first().map(|(i, _)| store.nodes[*i].id.clone()).unwrap_or_default()
    }

    #[test]
    fn an_exact_id_outranks_everything_else() {
        let s = fixture();
        assert_eq!(top(&s, "adopt-sutegi"), "adopt-sutegi");
        assert_eq!(top(&s, "sutegi"), "sutegi");
    }

    #[test]
    fn a_bare_pr_number_finds_the_pull_request() {
        let s = fixture();
        assert_eq!(top(&s, "7280"), "gh:join-com/data#7280");
        assert_eq!(top(&s, "data#7280"), "gh:join-com/data#7280");
    }

    #[test]
    fn words_from_a_title_match_across_terms() {
        let s = fixture();
        assert_eq!(top(&s, "violating content"), "adopt-sutegi");
        assert_eq!(top(&s, "zero-dep"), "sutegi");
    }

    #[test]
    fn a_type_filter_excludes_everything_else() {
        let s = fixture();
        let kinds = vec!["project".to_string()];
        let hits = s.search("sutegi", &kinds, 10);
        assert!(hits.iter().all(|(i, _)| s.nodes[*i].kind == "project"));
    }

    #[test]
    fn external_links_are_rebuilt_from_the_id() {
        let s = fixture();
        let link = |id: &str| s.link_of(&s.nodes[s.idx(id).unwrap()]).unwrap();
        assert_eq!(link("gh:join-com/data#7280"), "https://github.com/join-com/data/pull/7280");
        assert_eq!(link("gh:enekos/sutegi"), "https://github.com/enekos/sutegi");
        assert_eq!(link("svc:bai.hirusta.io"), "https://bai.hirusta.io");
        assert!(s.link_of(&s.nodes[s.idx("sutegi").unwrap()]).is_none());
    }

    #[test]
    fn traversal_stops_at_a_hub_instead_of_going_through_it() {
        let s = fixture();
        let pr = s.idx("gh:join-com/data#7280").unwrap();
        let (through, _) = s.neighbourhood(pr, 2, 500, 5000);
        let (stopped, held) = s.neighbourhood(pr, 2, 500, 25);
        // the author is reached either way; their other 40 PRs are not
        assert!(stopped.iter().any(|(i, _)| s.nodes[*i].id == "eneko-sarasola"));
        assert_eq!(stopped.len(), 3);
        assert_eq!(held, 1);
        assert!(through.len() > 40);
    }
}
