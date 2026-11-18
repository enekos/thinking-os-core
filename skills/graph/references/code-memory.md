# Code-aware temporal memory

Code memory connects a curated task to immutable git commits, the paths those
commits touched, and explicit test results. It indexes metadata and concise
conclusions — never diffs, commit bodies, tool calls, or agent transcripts.

## Selection is bounded

For every project with a `path:`, `sare extract git` selects:

- The latest **20 non-merge commits**. Set `history: N` on the project page to
  change this per repository, up to 200. `history: 0` disables recency-based
  selection.
- Every commit named by a task in that project, even when it is older than the
  recent window.
- Every commit named by a test result whose task belongs to that project.

Task and test references accept an unambiguous full or abbreviated commit SHA
of 7–64 hexadecimal characters. Branch names and mutable refs are deliberately
rejected. A referenced SHA is resolved to a full immutable SHA in the local
repository.

A commit node has id `commit:<repo-id>@<full-sha>`, its subject as the summary,
and its committer timestamp normalized to UTC in `updated`. A changed-path node has id
`path:<repo-id>:<repo-relative-path>` and the latest selected commit timestamp.
Paths may no longer exist at HEAD: they are temporal facts, not a file index.

The extractor reads `git show` metadata and `git diff-tree --name-only`. It does
not read file contents or diffs. Merge commits enter only when explicitly named.

## Task — what was intended

Copy `tasks/_example.md` to a slug-named page:

```yaml
---
title: Add scoped context
summary: "compile the decisions and code history needed for one coding objective"
type: task
status: active
updated: 2026-11-17
project: thinking-os-core
commits:
  - 9f86d08123456789abcdef0123456789abcdef0123
---
```

`project` creates `task PART_OF project`. Each entry in `commits` resolves to
`commit IMPLEMENTS task`; there is no issue-key or commit-message inference.
Add a SHA after the commit exists. Keep acceptance criteria and constraints in
the body, not in graph attributes.

## Attempt — what not to repeat

Copy `attempts/_example.md` when an approach is rejected or abandoned:

```yaml
---
title: Index every tool call
summary: "store complete agent transcripts as graph nodes"
type: attempt
status: rejected
updated: 2026-11-17
task: add-scoped-context
rejection: "transcript volume obscures durable decisions without adding useful evidence"
---
```

This creates `attempt ATTEMPTS task`. `summary` is the concise approach;
`rejection` is preserved in the node's `state` column. The body may name the
failing result or constraint. Do not paste a session log. Successful work is a
commit, not an attempt page.

## Test — what actually ran

Copy `tests/_example.md` after executing one command against one exact commit:

```yaml
---
title: Context compiler regression at 9f86d081
summary: "all context selection fixtures passed"
type: test
status: passed
ran_at: 2026-11-17T18:30:00Z
task: add-scoped-context
commit: 9f86d08123456789abcdef0123456789abcdef0123
command: "cargo test --manifest-path tools/Cargo.toml --test sare context"
exit_code: 0
---
```

Every result creates `test TESTS task` and `test TESTS commit`. Only
`status: passed` together with `exit_code: 0` also creates `VERIFIES` edges.
Failed and errored executions remain visible as test nodes without claiming
correctness. `command` and `exit_code` are preserved in `state`; `ran_at` is the
relation timestamp.

Only record `VERIFIES` for a clean worktree at the named commit. If the run
contained uncommitted changes, commit first and rerun, or record a non-verifying
result instead. Never infer an executed test from a changed test file or a
GitHub check name.

## Exemplar — code worth copying

Most tasks are bookkeeping. A few are code you would point the next engineer
(or agent) at: the clearest instance in the wiki of a pattern from the
engineering library. Those are exemplars, and the graph only holds the ones
that are earned.

Nominate a finished task with an `exemplar:` block:

```yaml
exemplar:
  concepts: [rust-type-driven-design]        # library concept pages it demonstrates
  why: "validated newtype ids make a cross-tenant write unrepresentable"
```

The block is a nomination. `sare extract git` writes `task EXEMPLIFIES concept`
only when every rule holds, and lists the rest in `health.md` §7 with the reason:

1. `status: done`.
2. `commits` names at least one SHA, and every one resolves.
3. Every commit is on the default branch (`origin/HEAD`, else `origin/main`,
   `origin/master`, `main`, `master`). For a squash or rebase merge, name the
   SHA that landed, not the branch SHA.
4. A passing test page (`passed`, `exit_code: 0`) for the task ran at a commit
   that contains all of them — the code that landed is the code that passed.
5. The commits touch at most `gate.max_exemplar_paths` paths (25). Larger
   changes are not readable as an example; nominate the smaller follow-up.

`EXEMPLIFIES` in `edges:` is refused: no page can declare itself an example.
`why` becomes the task node's `state`; `graph/views/exemplars.md` lists the
shelf by concept with commit links and the verifying test.

### When to nominate

The gate checks facts. Taste is the curator's, so nominate rarely — one
exemplar per concept per quarter is plenty. Nominate only when all are true:

- **It teaches a pattern.** Name the library concept and the exact move
  (a typestate, a bounded fan-out, an outbox). "Good code" is not a reason.
- **It is the clearest instance.** If an existing exemplar for the concept is
  as good, do not add a second.
- **A reviewer accepted it.** Merged through review, no follow-up fix needed
  in the same area since.
- **It reads alone.** Someone can open the commits and follow them without
  the PR thread: small, focused, named well, tested at the boundary.
- **It is not a workaround.** Hot fixes, migrations, generated code, vendored
  code, dependency bumps and config churn never qualify.

When unsure, do not nominate. A missing exemplar costs nothing; a mediocre one
teaches every later agent the wrong shape.

## Build and query

```bash
sare build local                 # pages + bounded git history; no GitHub/network
sare hop add-scoped-context -d 2 # task → commits/tests → changed paths
sare why path:gh:enekos/thinking-os-core:tools/src/sare/extract.rs
```

Useful relation slices:

```bash
grep -P '\tIMPLEMENTS\t' graph/edges.tsv
grep -P '\tTOUCHES\t' graph/edges.tsv
grep -P '\tVERIFIES\t' graph/edges.tsv
grep -P '\tEXEMPLIFIES\t' graph/edges.tsv
```

`IMPLEMENTS`, `ATTEMPTS`, `TESTS`, and `VERIFIES` are curator declarations with
provenance `page`; `EXEMPLIFIES` is a curator nomination that sare confirms. `CONTAINS` and `TOUCHES` come from git with provenance `git`.
Fusion still validates every endpoint and relation against the ontology.
