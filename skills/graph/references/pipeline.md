# The 9 stages, as implemented here

The discipline's pipeline, mapped onto the code that runs it. Model the domain
**before** extracting; fuse **before** storing; verify at every stage.

| Stage | Where it lives | Notes |
|---|---|---|
| 1 scope & value test | settled — see below | multi-hop queries, recurring entities |
| 2 representation | typed edges in TSV | property graph, not RDF, not a DB |
| 3 ontology | `graph/ontology.yaml` | node types + relations, domain and range |
| 4 entities | `sare extract pages/git/github` | nodes, one extractor per owner |
| 5 relations | same pass | edges, constrained to the ontology's list |
| 6 events | `zaindu` → `graph/events/` | GitHub transitions as an append-only ledger |
| 7 quality gate | `sare fuse` | domain/range, cardinality, danglers → `raw/rejected.tsv` |
| 8 fusion | `sare fuse` | alias map, conflict policy, node-attribute merge |
| 9 serve | `sare serve` | 4 markdown views + `hop`/`why` for agents |

## 1 — why a graph earns its keep

A graph pays off when queries are multi-hop, when entities recur across
documents, or when the relationships *are* the data:

- *"which of my projects has a PR waiting on a red check"* is
  project → repo → PR → checks — three tools, or one traversal.
- A shared library recurs across many project pages. Each said so in prose;
  as an edge it is one query.
- The value of a project catalog is which project blocks which. That is an
  edge set, not paragraphs.

A graph does **not** pay off for `briefings/` or `sources/` — append-only logs
read by date. They are nodes with almost no edges, and that is correct.

## 2 — representation

Property graph, stored as TSV. Chosen over the alternatives because:

- **RDF/Turtle** buys interop this graph has no consumer for.
- **SQLite or Neo4j** buys indexes a graph of a few thousand nodes does not
  need, and costs the thing that matters most: `git diff` on `wiki-edges.tsv`
  shows exactly which facts a change added or removed. Only the wiki's own
  claims (prov `page`/`legacy`) are tracked; the full `nodes.tsv`/`edges.tsv`
  are gitignored because most of them mirror GitHub, which keeps its own
  history, and they churn on every build.
- **TSV over JSON** because one fact per line is greppable
  (`grep -P '\tDEPENDS_ON\t' graph/edges.tsv`) and diffs line by line.

Time and provenance attach to every edge as columns: `since`, `prov`, `conf`,
`evidence`. Decided at stage 2 on purpose — retrofitting provenance after
extraction is how graphs become untrustworthy.

## 3 — ontology

Two rules, both enforced:

- **Every relation gets a precise verb.** The one vague relation, `MENTIONS`,
  is `deprecated: true`, carried at confidence 0.5, and counted as debt in
  `health.md`.
- **If two types are confused, keep them apart.** One wiki `project` type;
  GitHub repos are `repo`, never a second kind of project.

`DECIDES` vs `AFFECTS` exists for the same reason: one decision settles one
subject and constrains several others. One relation for both would make every
decision look like it settled five projects.

## 4-6 — extraction

Three extractors, one per source of truth. Each writes `graph/raw/<name>.json`
and none touches the TSVs, so fusion has exactly one place to validate.

| Extractor | Reads | Emits |
|---|---|---|
| `pages` | wiki frontmatter: `edges:`, `parent:`, `sources:`, `links:` | every wiki node; declared edges at conf 1.0; legacy links as MENTIONS at 0.5 |
| `git` | `path:` and `history:` on projects; task `commits`; test `commit` | repos and `LIVES_IN`; bounded timestamped commits and changed paths; `CONTAINS`, `TOUCHES`, declared `IMPLEMENTS`, `TESTS`, `VERIFIES` |
| `github` | `gh search prs` then `gh pr list --json` per repo | PR nodes with check status; `OPENED_IN`, `AUTHORED_BY`, `REVIEWS`, `DEPENDS_ON` for stacks |

Plus `graph/raw/watch.json`, written by `zaindu poll` — the only source that
still sees a PR after it merges.

Git history is selective: 20 recent non-merge commits per project by default,
plus older commits explicitly named by tasks and test results. Commit bodies,
diffs, file contents, tool calls and transcripts are never extracted. See
[code-memory.md](code-memory.md).

**An extractor that cannot reach its source emits nothing and says so.** It
never crashes the pipeline and never invents a node. A missing extractor makes
the graph smaller, not wrong.

No stage calls an LLM: structured sources (`gh --json`, `git`, YAML
frontmatter) need no model. An LLM would only be needed to extract from prose.

## 7 — the quality gate

`sare fuse` rejects, in order: unknown relation, dangling endpoint, domain
violation, range violation, self-edge, then many-to-one cardinality (keeping the
highest-confidence edge). Every rejection goes to `graph/raw/rejected.tsv`
**with its reason** and surfaces in `health.md`.

Rejections are never silently dropped. A growing pile means an extractor or the
ontology is wrong — or that pages carry genuinely broken `[[links]]`.

## 8 — fusion

- **Aliases** in `ontology.yaml` → `fusion.aliases` fuse the same real-world
  thing across surface forms: a GitHub login and a wiki person slug are one node.
- **Node attributes** merge non-destructively: a value a page declares beats a
  projected blank; a projected value fills a blank the page left.
- **Edge conflicts** resolve by `higher-conf-then-newer`; the loser's provenance
  is appended to the winner's `evidence`, so both are recorded.
- **Merges stay reversible** for the wiki's claims because `wiki-edges.tsv` is
  in git. Mirrored rows need no undo log: re-extracting rebuilds them.

## 9 — serving

- `index.md`, `graph/views/{work,projects,health}.md` — regenerated, never edited.
  `work.md` is PR-centric: your open PRs, review requests waiting on you, red
  checks named. "You" is `me.github` in `os.yaml`.
- `sare hop <node> -d N` — a bounded subgraph with provenance on every edge.
  Reach for it instead of grepping: it fits in context and says *how* two
  things connect, not just that they co-occur.
- `sare why <node>` — inbound edges. "How is this reachable", and names orphans.

## What is deliberately missing

- **Events beyond GitHub.** zaindu models PR transitions; nothing else is a
  first-class event. `log.md` and `briefings/` hold the rest as prose.
- **Embeddings and link prediction.** At this size, `grep` and a 2-hop
  traversal beat anything a model would infer.
- **LLM extraction from prose.** Dependencies stated in sentences become typed
  edges by hand. If that backlog grows past a few hundred, stage 4-6 gets an
  LLM pass — ontology in the prompt, span provenance on every triple, a 50-item
  hand check before anything is trusted. Not before.
