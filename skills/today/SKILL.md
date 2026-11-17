---
name: today
description: Generate today's prioritized briefing from the thinking-os — ranked by what should actually get done today. Reads the generated work/projects views, zaindu's GitHub and Linear proposals, active project pages and live git state. Triggers on "/today", "what should I do today", "what was I doing?".
---

# /today

Produce **today's briefing**. It lives at `<wiki>/briefings/YYYY-MM-DD.md` and
is also printed to chat. `<wiki>` is `$THINKING_OS_ROOT`, else `~/thinking-os`.

Treat this as bookkeeping, not chat. Terse, decisive.

## Procedure

### 1. Rebuild and reload

```bash
sare build          # pages + git + GitHub (+ Linear when enabled), then the views
```

Read, in order:

1. `<wiki>/CLAUDE.md` — conventions, if changed.
2. `graph/views/work.md` — your open PRs, review requests waiting on you, red
   checks named. This replaces opening GitHub. With Linear enabled it ends in
   **Tickets**: your open issues by state, each with the PRs attached to it —
   this replaces opening Linear.
3. `graph/views/projects.md` — status per project against **real git activity**;
   an active page whose repo has not moved in 45 days is flagged stale.
4. `graph/views/health.md` — anything the gate flagged. A stale active belongs in
   the briefing, not in a lint run.
5. `zaindu proposals` — the open boxes in `inbox/events/`: what GitHub and
   Linear did while you were away. A merged PR whose project's **Next up** is
   still unticked is a briefing item; so is a merge whose ticket is still in a
   pre-merge state (`check APP-12 (Code Review)`), a newly assigned or
   unblocked ticket, a priority raised to high/urgent, and a comment waiting on
   you.
6. Each `status: active` project page — its **Next up**.
7. Last 5 log entries: `grep "^## \[" log.md | tail -5`.
8. The most recent file in `briefings/` — what yesterday said, what's still open.

Do **not** re-derive from `gh` or Linear what these views already hold.

### 2. Probe live git, in one call

Uncommitted or recent work in active repos, quiet repos print nothing:

```bash
W="${THINKING_OS_ROOT:-$HOME/thinking-os}"
grep -l '^status: active' "$W"/projects/*.md | while read -r f; do
  grep -m1 '^path:' "$f" | sed 's/^path: *//'
done | while read -r p; do
  p="${p/#\~/$HOME}"
  git -C "$p" rev-parse --show-toplevel 2>/dev/null
done | sort -u | while read -r r; do
  [ "$r" = "$HOME" ] && continue
  st=$(git -C "$r" status -suno | head -15)
  lg=$(git -C "$r" log --oneline -5 --since='7 days ago')
  [ -z "$st$lg" ] && continue
  echo "### ${r##*/}  ($r)"
  [ -n "$st" ] && printf 'status:\n%s\n' "$st"
  [ -n "$lg" ] && printf 'recent:\n%s\n' "$lg"
  echo
done
```

If a page says "Next up: X" but git shows X landed, **update the page** (move X
to Recent log with the date). The briefing reflects reality.

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
- <only with Linear enabled — from work.md Tickets + zaindu proposals: newly
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
