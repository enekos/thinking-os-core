---
name: today
description: Generate today's prioritized briefing from the thinking-os — ranked by what should actually get done today. Reads the generated work/projects views, zaindu's GitHub and Linear proposals, active project pages and live git state. Triggers on "/today", "what should I do today", "what was I doing?".
---

# /today

Produce **today's briefing**. It lives at `<wiki>/briefings/YYYY-MM-DD.md` and
is also printed to chat. `<wiki>` is `$THINKING_OS_ROOT`, else `~/thinking-os`.

Treat this as bookkeeping, not chat. Terse, decisive.

## Procedure

### 1. Collect once

```bash
sare today --refresh   # rebuild, then collect bounded JSON; build logs go to stderr
```

Use `sare today` to reuse existing views offline when the user asks for a quick
briefing or cached data is sufficient. Inspect `source_freshness` either way;
report missing, stale or failed sources, never present cached data as current.
The collector returns work/projects/health excerpts, active project metadata,
deduplicated live git probes, unchecked proposals from the latest 30 inbox
files (at most 100), the last five log entries, and the latest briefing.
Missing files and truncation are explicit. It does not write briefing pages.

Do not repeat the collector's git probes or query GitHub/Linear for facts its
views already contain. Only expand an excerpt when a ranking decision needs it.

### 2. Read the shortlisted projects

Read the pages for the projects you are considering, especially **Next up**.
Reconcile claims against the collected git evidence. Read additional active
pages only if their metadata or proposals make them relevant to today's ranking.

GitHub PRs and assigned GitHub Issues work with the existing `gh` login;
Linear joins when configured. Both issue sources appear under **Tickets** in
work.md. Events include assignments, closure/reopening, priority changes,
unblocking and comments. A merged PR can name linked issues and their state.
Do not close or mutate a remote issue merely because a proposal mentions it.

### 3. Compile the briefing

Write `briefings/YYYY-MM-DD.md`; overwrite if it exists — `/today` regenerates.

```markdown
---
type: briefing
date: YYYY-MM-DD
---

# /today — YYYY-MM-DD

## TL;DR
<2–3 sentences. The single most important thing today.>

## Top priorities (ranked)
1. **<project>** — <concrete action>. _Why now:_ <one line>.
2. …

## Dispatched
- **<item>** → background agent; artifact at `<path>`. _(updated with the outcome)_

## GitHub
- <from work.md + zaindu proposals: what's red, what's waiting on you, what merged>

## Tickets
- <when either issue source has work — from work.md Tickets + zaindu proposals: newly
  assigned, unblocked, raised, commented; in-progress tickets with no PR>

## Watch list
- Not action-required today, worth watching.

## Deferred
- What you're explicitly NOT doing today, and why.
```

3–5 priorities. If you list 8, rank harder. Omit empty sections.

### 4. Dispatch the mechanical tail

Items with zero design judgment get a background agent, not a re-listing. Cap 3
per day. Apply the stop rule from the `graph` skill: only fan out over items
that never read each other's results.

- **Uncommitted pile** → stage and write a proposed message to
  `.git/PROPOSED_COMMIT` in that repo. Does **not** commit. Exception: the wiki
  itself — its bookkeeping is the OS's own state; commit it directly.
- **Wiki/reality drift** → reconcile the project page from `git log` (done items
  → Recent log with dates, `updated:` bumped), commit the wiki.
- **Runnable evidence** → run a test/eval whose exact invocation is already
  known; record where the report landed. Read-only on source code.

Put these rules in every dispatched agent's prompt verbatim:
- Never `git push`, never open PRs, never touch prod or staging.
- Never edit source code (staging is fine; changing is not).
- Commit only inside the wiki; elsewhere, `.git/PROPOSED_COMMIT` + staged tree.
- No `Co-Authored-By` trailers.

Anything needing a decision stays a ranked priority. Record each launch under
`## Dispatched`; update the line when the agent reports back.

### 5. Log and print

Append to `log.md`:

```
## [YYYY-MM-DD] today | <one-line summary>
```

Print the full briefing. Don't block on dispatched agents — print first, relay
outcomes as they arrive.

## Principles

- **Decisive over comprehensive.** A ranked 3 beats an exhaustive 12.
- **Dispatch, don't nag.** An item in 2+ briefings that needs no decision should
  have been dispatched.
- **Real over wishful.** No commit in 30 days → recommend dormant.
- **Honest about overload.** If Next up exceeds a day, say so and recommend deferrals.
- **No new pages.** `/today` updates project pages and writes the briefing;
  concept/decision pages are `/ingest`'s job.
