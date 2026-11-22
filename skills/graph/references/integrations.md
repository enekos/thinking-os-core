# GitHub as a graph

Linear, the optional second integration, has its own page: `linear.md`.

GitHub is already a graph. The job is projection, not scraping: `gh --json`
returns typed fields, so no stage parses rendered text.

## The extractor, via gh

`gh search prs --author=@me --state=open --json repository` finds the repos in
one call; `gh pr list --repo <nwo> --json` then gives per-PR detail. Useful
fields: `statusCheckRollup` (the red-check names that land in `work.md`),
`isDraft`, `headRefName`/`baseRefName`, `reviewRequests`.

Repos also arrive from project pages: a project whose `path:` resolves to a git
repo with a GitHub remote gets a `LIVES_IN gh:<owner>/<repo>` edge from the
`git` extractor, and that repo is polled too.

**Stacked PRs are an edge, not a convention.** When a PR's `baseRefName` is
another open PR's `headRefName`, it cannot land first — that is `DEPENDS_ON`,
extracted structurally rather than read off a description.

## Identity

"Mine" is decided by `me.github` in `os.yaml` (written by `install.sh` from
`gh api user`). `AUTHORED_BY` edges point at the login; to fuse the login with a
wiki person page, add the pair to `fusion.aliases` in the ontology — the only
place aliases belong.

## Failure modes

- **`gh` is not authenticated** — the extractor emits zero nodes and says so;
  the graph keeps the last fused state, so views go stale rather than empty.
  `gh auth login`.
- **`gh` is slow** — the GitHub pass dominates a full rebuild, one `gh pr list`
  per repo. `sare build pages` skips the network entirely and is the right
  command after editing frontmatter.
- **A merged or closed PR** disappears from `gh pr list --state open`. zaindu's
  `graph/raw/watch.json` keeps its title and state; without it the node would
  decay into a blank `not-open` placeholder.
- **Rate limits** — `gh` backs off and the extractor reports a partial pull.
  The next build fills the gap.

## GitHub Issues

GitHub Issues use the existing `gh` login and `me.github`; no Linear setup is
needed. `sare extract github-issues` reads assigned open issues and closed
issues updated in the last 14 days. Full `sare build` includes it by default;
`github_issues: false` in `os.yaml` opts out. Local/pages builds stay offline.

- Identity: `gh:<owner>/<repo>/issues/<number>`; PRs remain `gh:<owner>/<repo>#<number>`.
- Edges: issue `OPENED_IN` repo, `ASSIGNED_TO` owner, `SUBTASK_OF` parent,
  blocker `BLOCKS` issue, PR `IMPLEMENTS` issue from GitHub's own closing links.
  A title or branch mentioning `#123` is not proof of implementation.
- State: Open → `unstarted`, completed closure → `completed`, not-planned
  closure → `canceled`. Reopening returns to `unstarted`; GitHub Projects
  custom workflow fields are not inferred from labels.
- Priority labels: `urgent/high/medium/low`, `priority: <level>`, or `p0`–`p3`.
  Other labels do not imply priority.
- `zaindu poll` seeds Issues independently, then records assignment, state,
  completion, priority, unblocking and other people's latest comments. A missing
  issue does not imply closure. Own/bot comments do not request a reply.
- Failed/truncated reads retain the last complete snapshot. The work view,
  `sare today`, ikusi status board and artalde ticket links consume the result.

Scope is issues assigned to the configured owner, across repositories. Assign
an issue to yourself to include it. Polling does not create, close or comment
on remote issues. Use explicit user instructions for remote writes, then
`gh issue view <number> --repo <owner/repo>` and the appropriate `gh issue`
command. Link project pages to their repository with `LIVES_IN`; proposals file
GitHub Issue events under that project automatically.

Coding tasks can set `issue: gh:owner/repo/issues/123` (or a Linear id) to
create `task TRACKS issue`.

Existing wikis must add `issue` to the `OPENED_IN` domain, and allow
`TRACKS` from tasks to issues in their ontology;
compare `template/graph/ontology.yaml`. The installer never replaces a curated
ontology. New wikis receive this rule automatically.
