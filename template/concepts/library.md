---
title: Engineering library
summary: "start here: the curated engineering pages this wiki ships with — stack defaults, Rust and TypeScript practice, evaluation, agentic coding, systems — and when to read each"
type: concept
updated: 2026-11-17
---

# Engineering library

The reference shelf every new wiki starts with. Each page is a concept: terse,
opinionated, `EVIDENCED_BY` an immutable source under `sources/` where one
exists. **Read the page before the task**, not after the review.

## Read before you…

| …do this | Read |
|---|---|
| start a new app or service | [[stack-defaults]] |
| write or review any Rust | [[rust-style-guide]] |
| write or review TypeScript / Svelte | [[ts-architecture]] |
| ship an LLM feature, agent, benchmark, or trust agent findings | [[eval-methods]] |
| set up an optimisation or discovery loop | [[autoresearch]] |
| plan agent-heavy work, fan out, or design a prompt/skill | [[augmented-coding-patterns]] · [[graph-engineering]] |
| decide what to build next with no PM | [[inventing-work]] |
| build anything incremental (compiler, build tool, reactive cache) | [[query-system-design]] |
| deploy Rust to the edge | [[rust-on-workers]] |
| debug where an HTTP request's time goes | [[go-httptrace]] |
| change how this wiki works | [[llm-wiki]] · [[graph-engineering]] |

## By shelf

**Building — the default stack**
- [[stack-defaults]] — sutegi + SvelteKit + osagai; when to deviate.
- [[sutegi]] — the Rust web framework (project page: features, agent surface, gotchas).
- [[osagai]] — the Svelte 5 UI kit (project page: setup, what to reach for).

**Language practice**
- [[rust-style-guide]] — ownership, threads, `unsafe`, security, perf, lint baseline, agent mistakes.
- [[ts-architecture]] — feature modules, ports/fakes, parse at the edge, strict config, agent mistakes.
- [[rust-on-workers]] — workers-rs vs a raw Wasm ABI.
- [[go-httptrace]] — per-phase HTTP timing in Go.

**Measuring**
- [[eval-methods]] — fixtures, layered scoring, judge calibration, trajectories, refute-first, perf stats.
- [[autoresearch]] — the keep/revert loop and its read-only discovery variant.

**Working with agents**
- [[augmented-coding-patterns]] — a 51-pattern vocabulary for agentic coding.
- [[graph-engineering]] — typed edges with provenance; task graphs, not agent chains.
- [[llm-wiki]] — the split of labour this wiki runs on.

**Choosing work**
- [[inventing-work]] — 11 signals, ranked by leading/lagging × how pre-argued.

**Systems design**
- [[query-system-design]] — pull-based incremental computation, caching, cycle detection.

## Growing the shelf

- New material → `/ingest`: the raw copy lands in `sources/` (immutable), the
  takeaways go onto an existing page here or a new concept with
  `EVIDENCED_BY <source>`.
- Add the new page to the "Read before you…" table **and** a shelf. A page that
  isn't reachable from here won't be read.
- A practice page that has no source yet says so in its last line
  (see [[ts-architecture]]); ingesting one is a good first `/ingest`.
- Pages here are generic on purpose. Project-specific lessons go on the project
  page and link back to the rule they illustrate.
