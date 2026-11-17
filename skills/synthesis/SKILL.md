---
name: synthesis
description: Aggressively compact logs and noise across the thinking-os — fold old log.md entries into dense weekly/monthly summaries, trim bloated "Recent log" sections on project pages, collapse resolved Open questions, merge duplicate bullets. Cut hard; raw signal survives in sources/ and git. Triggers on "/synthesis", "compact the log", "the wiki feels noisy".
---

# /synthesis

Compress the wiki (`$THINKING_OS_ROOT`, else `~/thinking-os`; written `~/thinking-os` below). Over time `log.md` grows append-only, project-page "Recent log" sections accrete, and the same facts get restated across multiple bullets. This skill **rewrites for density** — same information, far fewer lines — so the wiki stays scannable in under 30 seconds per page.

Synthesis is **lossy on purpose**. Raw signal lives in `sources/` (immutable) and in git history (always recoverable). The wiki is the *curated* layer; this skill curates harder.

**Default posture: aggressive.** When in doubt, cut. The cost of an over-cut wiki is one `git checkout` to recover; the cost of an under-cut wiki is that nothing gets read. Lean toward the smaller file every time.

## Procedure

Be a careful editor about *facts*, not about *line count*. Never delete a fact that isn't preserved elsewhere — but assume most lines are noise until proven otherwise. **Propose the structural folds, then apply** (low-risk cuts like dropping narrative bodies and per-file LOC counts don't need a separate ask — bundle them into the apply step).

### 1. Pick a scope

The user invokes `/synthesis [scope]`. Scope is one of:

- **`log`** — compact `log.md` only.
- **`projects`** — compact `Recent log` / `Open questions` sections across `projects/*.md`.
- **`page <name>`** — compact a single page (e.g. `synthesis page my-app`).
- **`all`** (default if no arg) — log + projects.

If the user gives no scope and the file shapes don't obviously point at one, ask in a single line: "log, projects, all, or a specific page?"

### 2. Survey before editing

Read-only is cheap. Report sizes before touching anything:

```bash
wc -l ~/thinking-os/log.md
grep -c "^## \[" ~/thinking-os/log.md          # number of log entries
for f in ~/thinking-os/projects/*.md; do
  printf "%-40s %d lines\n" "$(basename "$f")" "$(wc -l < "$f")"
done | sort -k2 -n -r | head
```

Tell the user: "log.md is N lines / M entries; top-3 fattest project pages are X, Y, Z."

### 3. Compact log.md

**Rule of thumb**:
- Entries from **today + last 3 days**: leave untouched (still load-bearing for `/today` continuity).
- Entries **4–14 days old**: strip to **one line under the existing `## [date] op | subject` header**. No body paragraphs. The subject line is what `grep "^## \["` already surfaces; everything below it is dead weight.
- Entries **15–60 days old**: fold into a **weekly digest**. One `### Week of YYYY-MM-DD` header, then **3–8 bullets** covering the week, grouped by project. Drop day-level headers for that week.
- Entries **60+ days old**: fold weekly digests into a **monthly digest**. One `### YYYY-MM` header, then **3–6 bullets** covering the month's anchors only.
- **Bootstrap / one-off milestone entries** (wiki creation, mission pivots, major project starts/kills): preserve verbatim regardless of age. These are anchors.

**What stays**:
- Every `decision | ...` entry — these are pointers to ADRs; the rationale lives on the linked page anyway.
- Anything that names a still-open question or uncommitted state from the last 14 days.

**What gets cut hard**:
- `today | ...` lines older than 14 days — the briefings/ archive has the full version; the log doesn't need to duplicate it.
- Per-file LOC counts, test counts, exact byte sizes (unless they're the *point* of the entry).
- Step-by-step "I did X then Y then Z" narratives — collapse to "shipped X" or drop entirely if the artifact survives in git.
- Repeated mentions of the same project's status across consecutive entries — keep the latest, fold the rest.
- "Uncommitted" notes older than 3 days (either it landed, or check git and update the project page).
- Any bullet that restates what's already in the project page's Current state.
- Filler verbs and hedges ("started looking at", "explored", "thought about") — if nothing shipped, the entry probably doesn't need to survive at all.

Preserve the file shape: the header, the "Useful queries" block, the `---`, and the chronological ordering. Add a new line near the top after compaction:

```
_Last synthesised: YYYY-MM-DD — older entries folded into weekly digests below._
```

### 4. Compact project pages

For each `projects/*.md` page in scope:

- **Recent log**: cap at the **last 3 bullets**. Older bullets fold into a one-line `Earlier: …` summary at the bottom of the section, or drop entirely if (a) the same fact is now visible in Current state, or (b) git log already shows the work. Bias toward (b) — git is authoritative.
- **Open questions**: any question whose `DECIDES`/`AFFECTS` edge points at an existing decision page (`grep -P '\tDECIDES\t' ~/thinking-os/graph/wiki-edges.tsv`) → resolved, drop it (don't bother with a "Resolved" sub-list — the decision page is the record). Any question older than 30 days with no linked decision → flag in the proposal as a candidate for either resolution or removal.
- **Next up**: leave alone. Load-bearing for `/today` and is the user's curation, not noise.
- **Current state**: rewrite ruthlessly if you find contradictions with newer Recent-log bullets or with git. Current state should describe *what is true right now*, not the history of how it got there. Cut hedges, dates, and meta-commentary.
- **Frontmatter `updated:`**: bump to today's date if you changed anything substantive.
- **Page size target**: project pages should be under **150 lines**. If still over after compaction, flag in the report — the user may want to split or archive.

### 5. Inter-page deduplication (best-effort)

If the same fact appears verbatim on N pages (e.g. "auth goes through my-lib" on five pages), keep the canonical mention on the load-bearing page and replace the others with `[[backlinks]]`. This is fuzzy — propose, don't auto-apply.

### 6. Reporting format

Terse proposal — one block, then apply. Example:

```markdown
# /synthesis — 2026-06-02 — projected log.md 152→38, 3 pages compacted

log.md (152→38)
- 22 entries 4–14d → headers only, bodies dropped
- 8 entries 15–60d → folded into 2 weekly digests (3–8 bullets each)
- 5 entries 60d+ → folded into 2026-04 monthly digest
- preserved: bootstrap (1), decisions (3)

my-app.md (412→130) — recent log 11→3, dropped 2 resolved Open questions, rewrote Current state
my-lib.md (310→125) — recent log 8→3
my-site.md (281→140) — recent log 6→3, merged a duplicate mention

Out of scope: last 3 days, concepts/, decisions/, sources/, briefings/

Reply "go" or "show diff <file>".
```

Skip the proposal entirely for `synthesis log` when the file is over 250 lines — apply and report after, since the rules are mechanical.

### 7. Apply

On "go":
1. Edit each file via `Edit` (or `Write` for large rewrites — but only after reading the file in full).
2. Append a single entry to `log.md` **at the very end, under the chronological tail**, not inside the freshly-compacted region:

```
## [YYYY-MM-DD] synthesis | log.md 152→45 lines · 3 project pages compacted (my-app, my-lib, my-site)
```

3. One-line confirmation to user: "Compacted N files, M lines removed. Logged."

## What NOT to compact

- `sources/` — immutable raw material. Never touch.
- `briefings/` — point-in-time snapshots. Compacting them destroys the "what did I know on day X" record.
- `inbox/` — already ephemeral by design.
- `CLAUDE.md`, `README.md` — schema, not signal.
- `index.md`, `graph/` — generated output. Compacting it is pointless: the next `/graph build` overwrites it.
- `concepts/` and `decisions/` page bodies — they're already curated; if one of those gets bloated, that's a `/lint` finding, not a synthesis target.
- **Anything from the last 7 days** in any file. Recent context is still actively loaded by `/today`.

## Failure modes to avoid

- **Don't summarise away a still-open uncommitted state.** If a log entry says "uncommitted on branch X" and the work is still uncommitted (`git status` confirms), lift the fact into the project's Current state before dropping the log line.
- **Don't drop the only mention of a `[[link]]`.** Before compacting an entry that contains a `[[wiki-link]]`, grep for that link elsewhere; if this is the only mention, preserve it (or move the link into the project page).
- **Don't rewrite history.** Compacted text should be accurate to what happened, just shorter. Don't editorialise, reorder events, or merge two distinct ops into one bullet.
- **Don't compact during an active session.** If the user is mid-task with uncommitted work in the wiki itself, defer — synthesis is a quiet-time op, like `/lint`.

But **also avoid the opposite failure**: timid synthesis that leaves the file almost as long as it found it. If the proposal projects less than a 50% reduction on the targeted sections, the cuts aren't aggressive enough — reread and find more. The user can always `git checkout` to recover; they can't recover a wiki they stopped reading.

## When to call

- **Weekly** cadence. Aggressive synthesis only works if it runs often.
- When `log.md` exceeds **~100 lines or ~30 entries**.
- When a project page exceeds **~150 lines** and most of the bulk is Recent log.
- After any `/ingest` day that added more than 3 entries.
- Whenever the user says "the wiki feels noisy" or "I can't find anything in log.md anymore".
