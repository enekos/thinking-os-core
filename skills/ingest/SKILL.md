---
name: ingest
description: Ingest a new source (URL, paste, transcript, idea, screenshot) into the thinking-os — save the raw material, file takeaways onto relevant project/concept/decision pages with typed edges, rebuild the index and log it. Triggers on "/ingest", "file this", "process this".
---

# /ingest

Take a new source and integrate it into the wiki (`$THINKING_OS_ROOT`, else
`~/thinking-os`). This is where the wiki grows.

## Procedure

### 1. Get the source

`/ingest <something>`:

- **A URL** — fetch it, save cleaned markdown to `sources/YYYY-MM-DD-<slug>.md`, original URL in frontmatter.
- **A file path** — read it; copy into `sources/` if it isn't already in the wiki.
- **A paste** — save verbatim to `sources/YYYY-MM-DD-<slug>.md`.
- **An image** — view it, save the file to `sources/`, write a markdown sibling describing it.
- **No argument** — ask what to ingest.

`sources/` is **immutable**. Never edit a source after saving it.

### 2. Read and discuss

Extract 3–7 takeaways. **Tell the user what you found before filing anything.**
One short paragraph. Then either ask where it belongs or, if obvious, say
"Filing to X, Y, Z" and proceed.

### 3. Find related pages

```bash
sare hop <likely-node> -d 2       # what is near this topic, with typed edges
grep -rli "<term>" projects concepts decisions people
```

Treat `index.md`, `graph/` and `log.md` hits as noise — generated or exhaustive.
Real candidates are project, concept, decision and person pages.

### 4. File the takeaways

Update existing pages or create new ones; 5–15 files is normal.

- **Project page** — a Recent log bullet, maybe reorder Next up, maybe an Open question.
- **Concept page** — a cross-cutting idea used by several projects: `concepts/<slug>.md`.
- **Decision page** — a choice with rationale: `decisions/<slug>.md` with what, why, when, alternatives, consequences. Immutable once written; revisions `SUPERSEDES`.
- **Person page** — `people/<slug>.md`.

Every page you create or touch gets a one-line `summary:` and **typed `edges:`**
— `DECIDES`, `AFFECTS`, `APPLIES_TO`, `DEPENDS_ON`, `EVIDENCED_BY`, `INVOLVES` —
never a new bare `links:` entry. Read `graph/ontology.yaml` if unsure; an honest
untyped `[[link]]` in the body beats a wrong typed edge. Cite the source with
`EVIDENCED_BY <source-slug>` or the frontmatter `sources:` list.

### 5. Rebuild

```bash
sare build pages
```

`index.md` is generated from the summaries. **Never hand-edit it.** Check
`graph/views/health.md` for anything the gate rejected from your edits.

### 6. Log, commit, confirm

```
## [YYYY-MM-DD] ingest | <short title> → <pages touched>
```

Commit the wiki (pages + source + log). One line to the user: "Filed X.
Touched: <pages>. Logged."

## What not to ingest

- Tasks, not knowledge ("call X tomorrow") — that's a calendar item.
- Ephemeral chat with no lasting signal.
- Duplicates — find and update the existing page instead.

If it doesn't belong, say so and offer `inbox/` for capture without promotion.
