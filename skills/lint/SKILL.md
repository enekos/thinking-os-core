---
name: lint
description: Health-check the thinking-os — read the graph's health view, then find what the graph cannot compute: contradictions between pages, claims git contradicts, concepts mentioned but never written, active projects with empty Next up, stale frontmatter. Report findings and propose fixes. Triggers on "/lint", "health-check the wiki", "the wiki feels off".
---

# /lint

Audit the wiki (`$THINKING_OS_ROOT`, else `~/thinking-os`). Weekly, or whenever
it feels off. Quiet and thorough: run all checks, report once, **never bulk-edit
without asking**.

## 1. Read what the graph already computed

```bash
sare build pages
```

Then read `graph/views/health.md`: retyping debt, broken `[[link]]` targets,
orphans, stale actives, projects with no reachable repo, decisions with no
source, gate rejections. **Report these; do not re-derive any of them by hand.**

## 2. What only a reader can find

1. **Contradictions.** Read the most-edited 5–10 pages
   (`git log --since='30 days ago' --name-only --format= -- projects concepts decisions | sort | uniq -c | sort -rn | head`).
   Flag "X is the chosen approach" on one page vs "we chose Y" on another.
2. **Claims git contradicts.** For each active project, compare Current state /
   Next up against `git -C <path> log --oneline -10`. Done items still listed
   as next, or "shipped" claims with no commit.
3. **Concepts mentioned, never written.** `[[slug]]` links in bodies with no page —
   list as "worth writing", with reference counts.
4. **Active projects with an empty Next up.** `/today` has nothing to recommend.
5. **`updated:` older than the file's real last change.**
   `git log -1 --format=%cs -- <page>` vs the frontmatter date.
6. **Frontmatter gaps.** Every page needs `title`, `summary`, `type`, `updated`;
   projects also `status` and `path`.

## Report

```markdown
# /lint — YYYY-MM-DD

## From health.md
- 2 orphans, 0 dangling, 3 MENTIONS left (retype debt 4%)

## Found by reading
### Contradictions (1)
- `projects/a.md` says SQLite; `decisions/2026-10-01-storage.md` chose Postgres.
### Pages worth writing (2)
- `concepts/x.md` (referenced 3×)

## Proposed actions
1. …
Reply "go" to apply 1–2. 3 is your call.
```

## Apply

On assent, apply, `sare build pages`, commit, and log:

```
## [YYYY-MM-DD] lint | <N> issues found, <M> fixed; <list>
```
