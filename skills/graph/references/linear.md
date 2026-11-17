# Linear as a graph

Opt-in. Without a `linear:` key in `os.yaml` no extractor and no watcher ever
calls Linear, and none of this appears in the views.

```yaml
# os.yaml
me:
  github: octocat
linear:
  teams: [APP]      # optional
```

`linear: true` is enough. `teams` adds team keys to recognise in PR titles and
branches, on top of the teams your assigned issues already reveal — list one
when you open PRs for tickets that are not assigned to you.

Both tools go through the [`linear` CLI](https://github.com/schpet/linear-cli)
(`linear api`), which owns the credentials: `linear auth login`, or
`$LINEAR_API_KEY`. Everything is read-only.

## What is projected

One GraphQL query, shared by `sare extract linear` and `zaindu poll`: the
issues **assigned to you** that are open, plus the ones completed inside the
window (30 days for sare, `--since-days` for zaindu). Cancelled and duplicate
issues are never fetched.

| Node | id | Notes |
|---|---|---|
| `issue` | `APP-123` | `status` = Linear's state category, `state` = the team's name for it, `summary` = priority |
| `milestone` | `lin:project:<uuid tail>` | a Linear *project* — `project` is the wiki's own type |
| `cycle` | `lin:cycle:<team>-<n>` | summary is `start..end` |

Edges, all `prov: linear`: `SCOPED_TO` (issue → milestone), `PLANNED_IN`
(issue → cycle), `ASSIGNED_TO` (issue → you, the `me.github` person),
`SUBTASK_OF`, `BLOCKS`, and `IMPLEMENTS` (pr → issue) for every GitHub PR
Linear lists as an attachment of the issue. Nothing is inferred from text in
the graph: a ticket id in a branch name is not an edge.

## The one edge you write

A project page claims a Linear project:

```yaml
edges:
  - "TRACKS lin:project:76ed37275476"
```

Find the id with `sare hop APP-123` (the `SCOPED_TO` target) or
`grep milestone graph/nodes.tsv`. With that edge `projects.md` gains `tracks`
and `my open tickets` columns, and every issue event of that Linear project is
filed under the page (`· [[my-project]]` in the proposal line).

## What zaindu does with it

Issues live in the same snapshot as PRs and are diffed the same way:

| Event | Fires when | Box? |
|---|---|---|
| `issue.assigned` | an open issue appears in your list | yes |
| `issue.state_changed` | the state name changes (`Todo → In Progress`) | no — timeline |
| `issue.done` | it enters a `completed` state | yes — wiki bookkeeping |
| `issue.unblocked` | its last open blocker closes | yes |
| `issue.priority_raised` | priority rises to high or urgent | yes |
| `issue.commented` | the latest comment is newer and not yours | yes |

A `pr.merged` is annotated with its tickets — the issues the PR is attached
to, then ids read from its title and branch for known teams — each with the
state Linear has it in: `merged by @x · APP-12 (Code Review)`, proposed as
`…, check APP-12 (Code Review)`. A merge is when a ticket's state is most
likely to be stale.

Rules it keeps:

- **The first read seeds silently.** Turning Linear on never produces a burst
  of forty assignments.
- **A list it could not read in full is not diffed.** A failed or truncated
  fetch keeps the last full list and reports a warning; it never fires events
  and never marks the GitHub half of the poll incomplete.
- **A vanished issue is silence.** Reassigned, cancelled or aged out — none is
  work.

## Failure modes

- **The timer has no credentials.** launchd and systemd do not read your shell
  profile. `scripts/zaindu-poll.sh` sources `~/.config/thinking-os/env` when
  it exists; put `export LINEAR_API_KEY=…` there, mode 600. The symptom is
  `partial poll — linear: …` in `scripts/zaindu-poll.log` while a manual
  `zaindu poll` works.
- **`linear` is not installed or not authenticated** — `sare extract linear`
  emits nothing and says so; fusion keeps the last `graph/raw/linear.json`.
- **More than 400 assigned issues in the window** — pagination stops, sare
  reports a partial pull and zaindu keeps the last full list. Narrow
  `--since-days`.
