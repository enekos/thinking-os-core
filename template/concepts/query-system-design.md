---
title: Query system design (pull-based incremental compilation)
summary: "pull-based incremental compute engines (Krabby vs Salsa): tasks, queries, caching, 3-tier cycle detection"
type: concept
updated: 2026-11-17
sources:
  - sources/2026-08-05-krabby-query-system-design.md
edges:
  - "EVIDENCED_BY 2026-08-05-krabby-query-system-design"
---

# Query system design (pull-based incremental compilation)

Reference for demand-driven, incrementally cached compute engines: compilers,
build systems, eval/bench harnesses, LSPs. From Arya Dradjica's Krabby design,
benchmarked against [Salsa](https://github.com/salsa-rs/salsa).

## Push → pull
A push executor (A triggers B triggers C) is a query system without query
mechanics: no demand prioritization, poor cache use, breaks on unbalanced graphs.
Pull ("what do I need to answer this?") gives demand-driven priority — an LSP
asking for one hover shouldn't compile the whole program.

## Eight capabilities to design in from day one
- **Concurrency** — multi-threaded distribution + cross-thread cycle detection.
- **Asynchrony** — tasks pause/resume like futures instead of blocking a thread.
- **Batching** — N similar tasks together (Krabby: 64) for locality and SIMD.
- **Pre-emptive execution** — idle workers grab work by critical-path priority.
- **Whole-graph builds** — start dependents before dependencies finish; share memory, not disk.
- **Context-free incremental caching** — LRU, size-bounded, reusable across flags/commits.
- **Streaming queries** — return results as found ("find all references").
- **Accelerated testing** — reuse the machinery to re-run only affected tests.

## Architecture shape
- **Task** = state + output slot + `poll()` + priority (critical-path position).
- **Query** = dependency link; states initiated → pending/complete → blocked
  (cross-thread). Can stream partial results to a blocked dependent.
- **Workers** batch by task class and track parent/child for resumption.
- **Two-level cache:** in-memory live slots + on-disk interned task recordings.

## Incrementality across sessions
Red-green usually compares against one prior run. Walk *several* prior session
caches: a miss in the latest may still hit an older valid entry.

## Cycle detection under concurrency (3 tiers)
1. Fast per-thread eager check — catches most.
2. Lazy cross-thread reachability — catches what 1 misses.
3. Slow full graph walk — ground truth, rarely needed.

## When to reach for it
- Any tool with a task graph + cached results + "what's affected by this diff"
  (bench harnesses, eval runners, build tools). Affected-set + cross-run cache
  are the most transferable ideas.
- The futures-shaped task model hits the same `Send`-across-`.await` landmines
  the [[rust-style-guide]] flags for async Rust.

Back to the [[library]].
