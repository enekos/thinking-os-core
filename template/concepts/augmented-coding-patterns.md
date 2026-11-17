---
title: Augmented Coding Patterns
summary: "Lexler's taxonomy of AI-augmented coding (51 patterns, 10 anti-patterns, 15 obstacles), mapped onto this wiki's scaffolding"
type: concept
updated: 2026-11-17
sources:
  - sources/2026-06-01-augmented-coding-patterns.md
edges:
  - "EVIDENCED_BY 2026-06-01-augmented-coding-patterns"
---

# Augmented Coding Patterns

Lexler's catalog of AI-augmented coding. Full list in the source. The value here:
(a) **vocabulary** to audit how agents are used, (b) **gap-finding** — patterns not
yet in use that match a real pain, (c) **anti-pattern radar**.

## Already built into this wiki

### Approved Logs → `log.md`
- **One timeline across everything.** Pages drift and git is per-repo; `log.md`
  holds cross-repo ops, ingests and decisions that never touch a commit.
- **Grep-parseable.** `## [YYYY-MM-DD] <op> | <subject>` — `grep "^## \["`
  rebuilds the timeline; `grep ingest` slices by op.
- **Immutable past.** Entries are never rewritten; `/synthesis` folds old ones
  into digests but keeps the dated breadcrumb.
- **Feeds `/today`** — continuity across sessions.

### Approved Scenarios → frozen fixture corpora
- **Real inputs, frozen**, each with an expected output. Not synthetic.
- **Counter to non-determinism.** A prompt change is better only if more fixtures
  pass and none regress. One eyeballed example is not evidence.
- **Compounds.** Every real bug adds the fixture that would have caught it.
- **Forces a canary:** iterate on a ~10-fixture subset, full run before merge.
- See [[eval-methods]].

### The rest, one line each
- **Ground Rules** → `CLAUDE.md`.
- **Knowledge Document** → this wiki.
- **Context Management** → layout + frontmatter + typed edges; `sare hop` instead of reading whole files.
- **JIT Docs** → skill references loaded only when the skill runs.
- **Focused Agent / Orchestrator** → `/fan` task graphs ([[graph-engineering]]).
- **Extract Knowledge** → `/ingest` files takeaways onto pages.
- **Background Agent** → long agents run in the background.
- **Diagrams as Code** → `graph/views/*` and `sare viz`, generated from edges.
- **Feedback Loop** → [[autoresearch]].
- **Coerce to Interface** → tools emit JSON/TSV, not prose.
- **Text Native** → markdown all the way down.

## Worth adopting explicitly
- **Mind Dump + Check Alignment** — before a long task: dump everything, read the
  intent back, confirm. Catches silent misalignment before it costs a run.
- **Knowledge Checkpoint** — one line "what I think we're doing and why" between stages.
- **Cast Wide / Parallel Implementations** — for open design choices, 2–3
  candidates side by side instead of the first plausible one.
- **Canary in the Code Mine** — risky refactor on a small slice + regression run first.
- **Happy to Delete** — prune AI-generated cruft; counterweight to Sunk Cost.

## Anti-patterns to watch
- **Perfect Recall Fallacy** — verify memory against the repo before acting.
- **Sunk Cost** — a run that went sideways gets reverted, not patched.
- **Silent Misalignment** — `/lint` catches it late; Check Alignment catches it early.
- **Unvalidated Leaps** — no "done" without tests or a run.
- **AI Slop** — if a line doesn't change what the reader knows, delete it.
- **Distracted Agent** — keep sub-agent prompts narrow.

## Obstacles to design around
- **Context Rot** → trust git over memory.
- **Compliance Bias** → give a reviewer the job of pushing back.
- **Limited Focus** → Focused Agent decomposition.
- **Non-Determinism** → Approved Scenarios / regression gates.
- **Degrades Under Complexity** → Chain of Small Steps.

Back to the [[library]].
