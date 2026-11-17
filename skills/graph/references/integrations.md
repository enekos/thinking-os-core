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
