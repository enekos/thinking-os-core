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

The overview shows live graph totals and a neighborhood spotlight. Select a
node in the spotlight to explore it. The node directory browses the full graph
and accepts title, ID and summary searches. Collection links narrow it by type.

1. **Search reaches every surface form.** `42` finds `gh:owner/repo#42`. Ranking
   is id > title > summary, word boundary over mid-word, with a subsequence
   fallback and a type weight, so a project beats the PR that touched it.
2. **The node details list the whole neighborhood.** Every edge touching
   the node, grouped by verb and split by direction, each row carrying the other
   node's type, state and provenance, linking out to GitHub or the live host.
   The Wiki page tab renders markdown and navigable `[[links]]`.
3. **Neighborhood depth is adjustable.** Opening a node starts with its immediate
   neighbors. Choose two or three hops to expand further. Traversal stops at hubs:
   two hops from a PR reach its author, not the author's hundreds of other PRs.
4. **It reloads itself.** TSV mtimes are checked per request, so a `sare build`
   in another terminal lands in the open tab.

Large graphs open on a collection map. Line width reflects the number of
connections between types. Hover or focus a collection to highlight its links,
or select it to browse its nodes. **Individual nodes** opens the detailed map.
At a distance, numbered circles group nearby nodes. Select a circle to zoom in,
then select an individual node to open its neighborhood. The map controls zoom,
fit the visible nodes, and filter by type.

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
Individual node size reflects degree. In the static snapshot, disconnected
nodes are packed in a grid to the side.
