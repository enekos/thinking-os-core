---
name: graph
description: Operate the thinking-os knowledge graph and task graphs. Use when asked to build/rebuild the graph, query what connects to something, add or retype relations, check graph health, audit a page's edges, or design how agents fan out over a piece of work. Covers both halves of graph engineering — knowledge graph (ontology, extraction from pages/git/GitHub, fusion, serving) and task graph (diamond, stop rule, human gate). Triggers on "/graph", "rebuild the graph", "what depends on X", "why is X reachable", "retype the links", "graph health", "is this work splittable".
---

# graph — the thinking-os graph

thinking-os is a knowledge graph whose nodes are markdown pages and whose edges
are typed, dated and sourced. The wiki owns projects, tasks, rejected attempts,
test results, concepts, decisions, people, sources and briefings. GitHub owns
PRs and reviews. git owns repos and bounded commit/path history.

The wiki root is `$THINKING_OS_ROOT`, else the nearest parent holding
`graph/ontology.yaml`, else `~/thinking-os`. Paths below are relative to it.

**The schema is `graph/ontology.yaml`.** Read it before adding a relation.
Nothing enters the graph that does not validate against it, and that single
domain/range check is what stops this from becoming a word cloud with arrows.

## The pipeline

```
sare extract  →  graph/raw/*.json  →  sare fuse  →  nodes.tsv + edges.tsv  →  sare serve  →  views
 pages git           per source        gate + merge    (gitignored)                       index.md, work.md,
 github                                               wiki-{nodes,edges}.tsv (tracked)    projects.md, health.md
```

zaindu's `graph/raw/watch.json` is a fourth input to fusion — see
[references/zaindu.md](references/zaindu.md).

Never write a TSV by hand and never hand-edit a generated view. Edges come from
page frontmatter or a source system; views come from the graph.

## Commands

**`/graph` or `/graph build`** — the full rebuild:

```bash
sare build
```

Then report: node/edge counts, what the gate rejected, and what changed in
`git diff --stat`. The GitHub pass is most of the time.

`sare build pages` skips the network — use it after editing frontmatter.
`sare build local` rebuilds pages plus bounded git history without GitHub — use
it after naming a task commit or recording a test result.

**`/graph q <node>`** — the subgraph around a node, for your own context:

```bash
sare hop example-project -d 2
sare why example-project      # inbound edges: how is this reachable?
```

Prefer `hop` over grepping the wiki. It returns a bounded subgraph with
provenance on every edge instead of pulling whole pages into context.

**`/graph health`** — read `graph/views/health.md`: retyping debt, broken
targets, orphans, actives that git says are stale, projects with no reachable
repo, decisions with no source. Report the findings; propose fixes; ask before
bulk edits.

**`/graph retype [page]`** — drain the untyped-link debt:

```bash
sare retype            # dry run, every proposed conversion
sare retype --apply
```

It only converts what a stated rule types with confidence. `project -> project`
and `decision -> decision` need judgment: read both pages and choose between
`DEPENDS_ON`, `PART_OF`, `AFFECTS` and `SUPERSEDES` yourself, then write the
`edges:` entry. **Leaving a link untyped beats typing it wrong** — a wrong edge
silently corrupts every traversal that crosses it.

**`/graph add <src> <REL> <dst>`** — edit the source page's `edges:`
frontmatter, then `sare build pages`. Never edit a TSV.

**`/graph look`** — `ikusi` (or `ikusi open <node>`). See
[references/ikusi.md](references/ikusi.md).

## Writing edges on a page

```yaml
---
title: my-app
summary: "one line — the only copy; index.md is generated from it"
type: project
status: active
path: ~/src/my-app
edges:
  - "DEPENDS_ON my-lib"
  - "DEPLOYS_TO svc:app.example.com"
---
```

One `REL target` per line. Rules:

- **A precise verb or nothing.** The ontology has no `RELATED_TO` and will not get one.
- **Direction is part of the meaning.** A decision `DECIDES` a project; the project does not link back. Traversal goes both ways already (`sare why`).
- **`MENTIONS` is deprecated.** It only carries pre-ontology `links:` entries at confidence 0.5. Never create one.
- **A decision is immutable.** Revising a choice mints a new decision that `SUPERSEDES` the old one.
- **Every fact carries provenance.** `prov` and `conf` are set by the extractor, not by you — which is why declared edges beat projected ones on conflict.

## Task graphs

The other half: how agents work, not what they remember. Read
[references/task-graphs.md](references/task-graphs.md) before dispatching more
than one agent, and apply it to `/today`'s dispatch step and `/fan`.

- **Ask where the work splits into pieces that never read each other's results.** Split only that. Everything sequential stays with one agent.
- **The diamond**: plan → parallel workers → verify in a *separate context* → one owned merge.
- **The human gate sits on irreversible edges only** — push, deploy, publish, delete.
- **Caps**: one writer per file, a round limit on every loop, a hard cap on spawned agents (3 for `/today`).

## Code-aware memory

Read [references/code-memory.md](references/code-memory.md) before creating a
task, rejected attempt, test result, or changing git history extraction. It
defines the bounded selection rule, exact-commit verification, and why diffs,
tool calls and transcripts stay out of the graph.

## Reference files

- [references/task-graphs.md](references/task-graphs.md) — fake edges, the diamond, the stop rule, the human gate, guardrails.
- [references/pipeline.md](references/pipeline.md) — the 9 stages as implemented: what each extractor reads, how fusion resolves conflicts.
- [references/code-memory.md](references/code-memory.md) — tasks, attempts, exact-commit tests, and bounded commit/path history.
- [references/integrations.md](references/integrations.md) — GitHub as a graph: the queries, stacked PRs, failure modes.
- [references/linear.md](references/linear.md) — Linear, opt-in: the `linear:` key, issue/milestone/cycle nodes, `TRACKS`, the issue events.
- [references/ikusi.md](references/ikusi.md) — the viewer on `127.0.0.1:4747`, and `sare viz` for a static file.
- [references/zaindu.md](references/zaindu.md) — the GitHub and Linear event watcher: searches, ledger, inbox proposals, the two fields it re-derives.
- [references/code.md](references/code.md) — the crate, its three binaries, install and test commands.

## Credits

The discipline, the 9-stage pipeline and the task-graph rules come from
[codejunkie99/graph-engineering](https://github.com/codejunkie99/graph-engineering)
(MIT, see `references/LICENSE-graph-engineering`), itself a distillation of
Southeast University's graduate Knowledge Graph course
([npubird/KnowledgeGraphCourse](https://github.com/npubird/KnowledgeGraphCourse),
Prof. Peng Wang) plus Google DeepMind × MIT's
["Towards a Science of Scaling Agent Systems"](https://research.google/blog/towards-a-science-of-scaling-agent-systems-when-and-why-agent-systems-work/).
