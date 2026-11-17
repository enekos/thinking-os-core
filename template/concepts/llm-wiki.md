---
title: LLM wiki
summary: "the pattern this wiki runs on: the LLM files and links, the human reads and curates"
type: concept
updated: 2026-11-17
edges:
  - "APPLIES_TO example-project"
---

# LLM wiki

A personal knowledge base the LLM maintains on the owner's behalf.

- **Split of labour.** The LLM does the bookkeeping — saving sources, filing
  takeaways onto pages, linking, rebuilding indexes, compacting logs. The human
  reads, decides and asks.
- **Sources are the floor.** Raw material lands in `sources/` verbatim and is
  never edited. Every page above it is a curated, lossy synthesis that can be
  re-derived.
- **Typed edges, not prose links.** "Built on X" in a sentence is invisible to
  a query; `DEPENDS_ON x` in frontmatter is a fact with a date and provenance.
  The ontology rejects edges whose endpoints don't fit, so the graph stays a
  graph instead of a word cloud.
- **Generated views, never hand-kept catalogs.** A catalog maintained by hand
  drifts; one generated from each page's `summary:` cannot.
- **Git over the page.** A page claiming "active" is checked against real
  commit activity; when they disagree, the page is wrong.
- **Lossy on purpose.** `/synthesis` compacts logs hard because an unread wiki
  is worthless; git and `sources/` keep everything recoverable.
