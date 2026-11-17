# thinking-os

A personal knowledge graph kept by an LLM. You read, curate and ask; the LLM
files, links and rebuilds.

- **Pages are nodes** — `projects/`, `concepts/`, `decisions/`, `people/`,
  `sources/`, `briefings/`. Markdown with YAML frontmatter.
- **Edges are typed** — `edges:` in frontmatter, validated against
  `graph/ontology.yaml`. Git repos and GitHub PRs join the graph automatically.
- **Views are generated** — `index.md`, `graph/views/{work,projects,health}.md`.
  Never edit them; edit a page and run `sare build pages`.

## Daily use (in Claude Code, from this directory)

| Command | Does |
|---|---|
| `/today` | ranked briefing from your PRs, projects and git |
| `/ingest <url\|paste\|file>` | file new material onto the right pages |
| `/graph` | rebuild, query (`sare hop <node>`), health |
| `/lint` | contradictions and drift the graph can't see |
| `/synthesis` | compact the log and project pages |
| `/fan <work>` | split work across agents — only where it splits |

Look at the graph: `ikusi` (opens `http://127.0.0.1:4747`).

GitHub changes arrive every 5 minutes as proposals in `inbox/events/`
(`zaindu proposals`). Nothing edits a page on its own.

Engine, install and upgrades: the **thinking-os-core** repo (`/setup`).
