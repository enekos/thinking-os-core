# thinking-os — operating schema

This directory is the owner's **personal thinking OS**: a knowledge graph the
LLM maintains on their behalf, holding projects, decisions and context. Nodes
are markdown pages; edges are typed, dated and sourced. The LLM writes; the
owner reads, curates and asks questions.

Read this before touching anything here. The schema is `graph/ontology.yaml`;
the operations, the viewer (ikusi), the event watcher (zaindu) and the code
live in the **`graph` skill** — load it before building, querying or changing
the pipeline. The engine lives in the separate **thinking-os-core** repo; this
directory is data only.

This file loads into every session started here — keep it short. Detail
belongs in a skill reference.

## Layout

```
CLAUDE.md  README.md  os.yaml (me.github, linear)  log.md (append-only)  index.md (GENERATED)
graph/      ontology.yaml · services.tsv (hand-kept) · wiki-{nodes,edges}.tsv (GENERATED, tracked)
            nodes.tsv/edges.tsv (GENERATED, gitignored — full graph incl. GitHub)
            events/YYYY-MM.tsv (zaindu ledger) · raw/ (gitignored) · views/ (GENERATED)
projects/ tasks/ attempts/ tests/       project + coding-memory node bodies
concepts/ decisions/ people/            curated knowledge node bodies
briefings/  past /today output       sources/  IMMUTABLE ingested material
inbox/      capture; inbox/events/ = zaindu proposals
scripts/    zaindu-poll.sh (timer, every 5 min), autocommit.sh (hourly)
```

`output/` and `reports/` are gitignored agent scratch — file the conclusion
into a page, not the evidence.

## Rules

- **Never hand-edit a generated file** (`index.md`, `graph/views/*`,
  `graph/*.tsv`). Edit frontmatter, then `sare build pages` (no network) or
  `sare build` (pages + git + GitHub).
- **Don't read generated files whole.** `sare hop <node> -d 2`, `sare why <node>`
  and `grep` on one relation answer most questions in a fraction of the tokens.
  `git diff` shows generated files as binary on purpose (`.gitattributes`).

### Page frontmatter

```yaml
---
title: <human-readable title>
summary: "<one line — the ONLY copy; index.md is generated from it>"
type: project | task | attempt | test | concept | decision | person | source | briefing
status: active | blocked | dormant | done | unknown   # projects
updated: YYYY-MM-DD
path: <local repo path>                                # projects
edges:
  - "DEPENDS_ON my-lib"
sources:
  - sources/<filename>.md
---
```

### Edges — in order of damage when broken

1. **A precise verb or nothing.** No `RELATED_TO` exists. Can't tell
   `DEPENDS_ON` from `PART_OF`? Read both pages; still unsure → leave a
   `[[link]]` in the body. An honest untyped link beats a wrong typed one.
2. **Direction is meaning.** A decision `DECIDES` a project; no back-link —
   traversal goes both ways already.
3. **`MENTIONS` is deprecated.** Never create one.
4. **Decisions are immutable.** A revision is a new decision that `SUPERSEDES`.
5. **Sources are immutable.** The provenance floor.

### Code memory

- Copy `tasks/_example.md`, `attempts/_example.md`, `tests/_example.md`; never
  edit the underscore examples into real records.
- Projects keep the latest 20 non-merge commits by default (`history: N`).
  Task `commits` and test `commit` SHAs retain older relevant commits.
- `TESTS` records an execution; only `passed` + `exit_code: 0` earns `VERIFIES`.
  Rejected attempts need a concise `rejection:`.
- No diffs, commit bodies, tool calls, or transcripts. Run `sare build local`.
- `exemplar:` on a done task nominates it as code worth copying; sare emits
  `EXEMPLIFIES` only when it is merged, test-verified and small. Nominate
  rarely — the rubric is in the graph skill's `references/code-memory.md`.

## Library and stack

`concepts/library.md` is the engineering shelf — read the page **before** the task:

- Rust → `concepts/rust-style-guide.md` · TypeScript/Svelte → `concepts/ts-architecture.md`
- Rust types/API → `concepts/rust-type-driven-design.md` · Rust resources/concurrency → `concepts/rust-resource-and-concurrency-patterns.md`
- TypeScript domain → `concepts/typescript-domain-patterns.md` · async/resilience → `concepts/typescript-async-and-resilience-patterns.md`
- Test design or a bug fix → `concepts/verification-patterns.md`
- Code review, architecture checks, or quality gates → `concepts/architecture-fitness-and-quality-gates.md`
- LLM feature, agent harness, benchmark, or agent findings → `concepts/eval-methods.md`
- **New app or service → `concepts/stack-defaults.md`**: a sutegi server
  (Rust, std-only, SQLite → Postgres) serving a SvelteKit SPA built on the
  osagai kit (`projects/sutegi.md`, `projects/osagai.md`). Kit components and
  `query()`/`task()` before hand-rolled UI. A gap in sutegi or osagai is fixed
  upstream, not patched in the app; any deviation gets a decision page.

## Operations — each is a skill

`/graph` (build, query, health, retype) · `/ingest` · `/today` · `/lint` ·
`/quality-gate` · `/fan` · `/synthesis` · `/setup`. The skill holds the steps.
`/lint` reads `graph/views/health.md` first and never recomputes it.
`/quality-gate` checks product code and exact-commit test evidence, not wiki health.

## Log format

`log.md` is append-only, one grep-parseable header per op:

```
## [2026-11-17] ingest | <title> → concepts/x.md, projects/y.md
```

## Tone and execution

- **Curator, not chatbot.** Do the bookkeeping silently; report what's interesting.
- **Terse pages.** Bullets over prose, state over story. No AI slop: if a line
  doesn't change what the reader knows, delete it.
- **Trust the graph over the page, git over both.** A page saying active while
  `graph/views/projects.md` shows 60 idle days is wrong — fix the page.
- **Don't invent state.** Unknown → `status: unknown`.
- **Respect dormant.** `projects/dormant.md` is the catch-all; promote on real activity.
- **Fan out only where work splits** into pieces that never read each other's results.
- **Commit small.** Commit pages after each ingest. Ledgers and generated files
  commit themselves hourly (`scripts/autocommit.sh`, run by the zaindu timer).
