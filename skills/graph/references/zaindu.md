# zaindu — the graph reacts to GitHub

**zaindu** (Basque: *to watch over*; `tools/src/watch/`) turns GitHub state
changes into graph input. A timer runs `scripts/zaindu-poll.sh` every 5
minutes — a systemd user timer on Linux (`thinking-os-zaindu.timer`), launchd
on macOS. It is read-only against GitHub, writes nothing outside the wiki, and
sends no notifications.

Three searches, one snapshot, a diff:

```
my open PRs  +  my merged PRs  +  PRs awaiting my review
        │
        └─ diff vs scripts/.gh-watch-state.json  (gitignored)
              ├─ graph/events/YYYY-MM.tsv  the ledger — append-only, one file a month
              ├─ inbox/events/<date>.md    proposals — one unchecked box each
              └─ graph/raw/watch.json      a fusion source, like any extractor
```

"My" is `me.github` in `os.yaml`.

Seven event kinds: `pr.merged`, `pr.approved`, `pr.changes_requested`,
`pr.checks_red`, `pr.checks_green`, `pr.closed`, `pr.review_requested`.

```bash
zaindu poll --dry-run     # what would fire, touching nothing
zaindu poll --seed        # first run: record the current state, emit nothing
zaindu events --kind merged
zaindu proposals          # today's open boxes
zaindu status             # the watched set
```

After each poll, `scripts/autocommit.sh` commits the machine-written paths
(ledgers, generated views, `wiki-*.tsv`) at most once an hour, only on the
default branch, never pushing. Your pages and staged work are left alone.

Three rules this obeys:

1. **An event proposes; it never edits.** A box in `inbox/events/` is work for
   `/today` to rank or for you to do. Nothing in the wiki changes on its own.
2. **`watch` is the only source that sees a PR after it closes.**
   `sare extract github` lists open PRs only, so a merged PR would decay into a
   blank placeholder. Fusion reads `graph/raw/watch.json` for exactly that.
3. **Two GitHub fields lie, so zaindu derives both.** `statusCheckRollup.state`
   stays `FAILURE` after a failed check is re-run green, and `reviewDecision` is
   null unless branch protection requires a review. Red is computed per check
   name (a name with a later `SUCCESS` is not failing) and approval is read off
   `latestReviews`. Both are covered by tests in `tools/src/watch/gh.rs`.
