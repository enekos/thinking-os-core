# ikusi — the graph viewer

**ikusi** (Basque: *to see*) is the graph viewer: a `sutegi` server on
`127.0.0.1:4747` that reads `graph/nodes.tsv` and `graph/edges.tsv`.

```bash
ikusi                      # start it if it is down, then open the browser
ikusi open example-project # the same, focused on one node
ikusi open 'gh:owner/repo#42'
ikusi serve --port 4747    # foreground, for a log you can watch
ikusi status               # is it answering?
```

The browser is opened with `xdg-open` on Linux and `open` on macOS. On a
headless box, run `ikusi serve` and forward the port
(`ssh -L 4747:127.0.0.1:4747 <host>`).

Four things it does that a rendered file cannot:

1. **Search reaches every surface form.** `42` finds `gh:owner/repo#42`. Ranking
   is id > title > summary, word boundary over mid-word, with a subsequence
   fallback and a type weight, so a project beats the PR that touched it.
2. **The sidebar is the whole neighbourhood, with links.** Every edge touching
   the node, grouped by verb and split by direction, each row carrying the other
   node's type, state and provenance, linking out to GitHub or the live host.
   Wiki pages render their markdown body underneath, `[[links]]` included.
3. **It focuses instead of fitting.** The default view is the node's
   neighbourhood, and traversal **stops at a hub rather than going through it**:
   two hops from a PR reach its author, not the author's hundreds of other PRs.
4. **It reloads itself.** TSV mtimes are checked per request, so a `sare build`
   in another terminal lands in the open tab.

Keys: `⌘K`/`Ctrl-K` search, `↩` focus, `⌘↩` open the source, `⌘E` open the page
locally, `[` / `]` walk history, `1` / `2` / `3` depth, `g` whole graph, `?` help.

**Status** (`s`, or `/#/status`): the latest open PRs I opened, the ones waiting
on my review, and my Linear issues split into started and not started — newest
first. It reads zaindu's snapshot (`scripts/.gh-watch-state.json`), not the
graph, so it is as fresh as the last poll. `GET /api/status?limit=N` is the same
data as JSON.

`sare viz` writes `graph/views/graph.html` — one self-contained page, no network,
no server — and `--dot` a graphviz file. Gitignored: rebuild it, don't diff it.

```bash
sare viz                  # the static snapshot
sare viz --dot            # graphviz instead
```

Colour is the **world** a node belongs to — this wiki, or code & runtime — and
**shape** is its type within that world, so type never rides on colour alone.
Node size is degree. Disconnected nodes are packed in a grid to the side.
