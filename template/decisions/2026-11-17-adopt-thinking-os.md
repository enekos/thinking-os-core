---
title: adopt thinking-os
summary: "run personal project knowledge as an LLM-kept typed graph instead of loose notes"
type: decision
status: accepted
updated: 2026-11-17
edges:
  - "DECIDES example-project"
  - "AFFECTS llm-wiki"
---

# adopt thinking-os — 2026-11-17

**What.** Keep project state, decisions and ingested material in this wiki, as
pages with typed edges, maintained by an LLM through the `/ingest`, `/today`,
`/lint`, `/synthesis` and `/graph` skills.

**Why.** Loose notes answer "what did I write" but not "what depends on this"
or "what is red right now". A typed graph joined to git and GitHub answers both
without opening three tools.

**Alternatives.** A notes app (no typed edges, no git/GitHub join); a database
(not diffable, not greppable, not markdown).

**Consequences.** Every new page needs a `summary:` and typed `edges:`. Views
are generated — never edited. Decisions like this one are immutable; a change
of mind is a new decision that `SUPERSEDES` it.

Decisions should carry an `EVIDENCED_BY <source>` edge — `health.md` counts
the ones that don't. This example has none on purpose: ingest something and
add one.
