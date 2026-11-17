---
title: graph engineering
summary: "the discipline this wiki runs on — typed edges with provenance instead of untyped links, and task graphs instead of agent chains"
type: concept
updated: 2026-11-17
sources:
  - sources/2026-09-16-graph-engineering.md
edges:
  - "EVIDENCED_BY 2026-09-16-graph-engineering"
---

# graph engineering

**Design the structures agents work through, not the prompts.** Two halves:
knowledge graphs (what agents remember) and task graphs (how agents work). In
this wiki the schema is `graph/ontology.yaml`, the pipeline is `sare`, and the
operations are the `graph` skill.

## The failure modes it fixes

| Symptom | Diagnosis |
|---|---|
| Hundreds of untyped `[[links]]` | No ontology — "related" without saying how is a word cloud with arrows. |
| One project's state in the index, a board **and** its page | No fusion — three copies of one node, drifting. |
| A tracker scraped from a terminal table | A graph flattened to a screenshot. Project it from the API instead. |
| Dumped pages with no inbound edges | A dump, not a projection: no identity, no edges. |
| Broken links nobody catches | No quality gate validating edge endpoints. |
| planner → coder → tester → reviewer | A sequential chain: the one topology where multi-agent setups lose. |

## How this wiki applies it
- **Ontology first.** Every relation has domain and range; `RELATED_TO` does not
  exist. A type-incompatible edge is rejected at `sare fuse`.
- **Provenance on every fact.** `since`, `prov`, `conf`, `evidence` on each edge;
  declared beats inferred, both are recorded.
- **Projected, not scraped.** GitHub comes in through `gh --json`; git through the
  repo itself. Both are external node types with no page.
- **Generated catalogs.** `index.md` and `graph/views/*` come from the graph, so
  they can't drift.
- **Quality gate is a number.** `graph/views/health.md`: retyping debt, broken
  targets, orphans, stale actives.

## Two rules worth remembering
- **Model before extracting.** Writing pages first and asking what they mean
  later produces every symptom above.
- **The shape of the work decides.** Before fanning out agents, find where the
  work splits into pieces that never read each other's results — split only
  there (`/fan`). Coordinated teams win big on splittable work and lose on
  sequential work. More agents is not a strategy.

See also: [[llm-wiki]] · [[augmented-coding-patterns]].

Back to the [[library]].
