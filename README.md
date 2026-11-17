# thinking-os-core

The engine behind a **thinking-os**: a personal knowledge graph an LLM keeps on
your behalf. Pages are nodes, frontmatter edges are typed and validated against
an ontology, git repos and GitHub PRs join the graph automatically, and Claude
Code skills do the bookkeeping.

This repo is the engine only. Your wiki is a separate git repo, created from
`template/` on install and never touched by upgrades.

## Layout

```
tools/       one Rust crate, three binaries
               sare    pipeline: extract (pages, git, github) · fuse · serve · retype · viz
               zaindu  GitHub watcher: PR transitions → ledger + inbox proposals
               ikusi   graph viewer on 127.0.0.1:4747
template/    the wiki skeleton: CLAUDE.md, ontology, example pages, scripts/
skills/      Claude Code skills: setup, graph, today, ingest, lint, synthesis, fan
deploy/      systemd user units (Linux), launchd plist (macOS) for the 5-min poll
install.sh   build, create/upgrade the wiki, link skills, enable the timer
```

## Quick start

Needs git, `gh` (authenticated), and Rust ≥ 1.85.

```bash
gh repo clone enekos/thinking-os-core ~/src/thinking-os-core
~/src/thinking-os-core/install.sh            # wiki at ~/thinking-os
```

Or open Claude Code in the clone and run `/setup` — it preflights the machine,
runs `install.sh`, and verifies every piece.

Flags: `--wiki DIR`, `--no-timer`, `--skills-dir DIR`. Re-running upgrades:

```bash
git -C ~/src/thinking-os-core pull --ff-only && ~/src/thinking-os-core/install.sh
```

Then, from the wiki directory in Claude Code: `/today`, `/ingest <url>`,
`/graph`, `ikusi`.

## Tests

```bash
cargo test --manifest-path tools/Cargo.toml
bash tools/e2e-zaindu.sh      # offline event chain, then a live GitHub leg
```

## Left out, compared to the original

- **Linear** — no issue/milestone/cycle nodes, no ticket relations; `work.md` is
  PR-centric.
- **The frozen Python oracle** and its byte-identical differential test.
- **Personal tooling** — go-live watchdogs, notification hooks, machine-specific
  paths. zaindu sends no notifications.
