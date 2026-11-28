# thinking-os-core

The engine behind a **thinking-os**: a personal knowledge graph an LLM keeps on
your behalf. Pages are nodes, frontmatter edges are typed and validated against
an ontology, git repos, bounded commit/path history, GitHub PRs, assigned GitHub Issues and (opt-in)
Linear issues join the graph automatically, and Claude Code skills do the bookkeeping.

This repo is the engine only. Your wiki is a separate git repo, created from
`template/` on install and never touched by upgrades.

## Layout

```
tools/       one Rust crate, three binaries
               sare    pipeline: extract (pages, git, github, github-issues, linear) · fuse · serve · retype · viz
               zaindu  watcher: PR and Linear issue transitions → ledger + inbox proposals
               ikusi   graph viewer on 127.0.0.1:4747
artalde/     the agent control plane: an egui window over every coding agent on the machine
template/    the wiki skeleton: CLAUDE.md, ontology, coding-memory examples, scripts/,
               and the engineering library (below)
skills/      Claude Code skills: setup, graph, today, ingest, lint, quality-gate, security-audit, synthesis, fan, artalde
deploy/      systemd user units (Linux), launchd plist (macOS) for the 5-min poll
install.sh   download a release, create/upgrade the wiki, link skills, enable the timer
packaging/   release bundling, the update command, and offline installer tests
RELEASING.md maintainer release and recovery instructions
```

## Quick start

Needs git, `gh` (authenticated), `curl`, and tar. No Rust toolchain is needed.
Prebuilt releases currently support Linux x86_64 and macOS Apple Silicon.

```bash
curl -fsSL https://github.com/enekos/thinking-os-core/releases/latest/download/install.sh | bash
# wiki at ~/thinking-os
```

To choose a wiki or disable optional pieces, pass the installer flags through
`bash -s --`, for example `bash -s -- --wiki ~/wiki --no-timer --no-artalde`.
The installer downloads the matching release bundle, verifies its SHA-256
checksum, and installs the tools in `~/.local/bin`.

Or open Claude Code in a source clone and run `/setup` — it preflights the
machine, runs the installer, and verifies every piece. Contributors can build
the checkout instead with `./install.sh --from-source`.

Keep the engine and skills current with one command:

```bash
thinking-os update          # download and activate the latest release
thinking-os update --check  # check without changing the installation
thinking-os version         # installed release and target
```

The update command remembers the wiki, skills directory, timer, and artalde
options from installation. Releases are unpacked under
`~/.local/share/thinking-os-core/versions/` and activated through a `current`
symlink; a failed download or checksum leaves the active release unchanged.
Existing wiki pages are never overwritten. New template pages are added, while
changes to `CLAUDE.md` and `graph/ontology.yaml` are reported for manual review.

Then, from the wiki directory in Claude Code: `/today`, `/ingest <url>`,
`/graph`, `ikusi`.

## artalde — the agent control plane

`artalde` is a native window (egui) over the coding agents running on this
machine, in the spirit of [herdr](https://github.com/herdrdev/herdr-app): it
shows which agents need you, runs them in its own terminals, and reviews their
pull requests.

- **Terminals** — Ghostty's emulator (`libghostty-vt`) in split panes and
  tabs. A small host process owns the programs, so they outlive the window;
  `artalde term run|ls|read|send` drives them from a shell or an agent.
  Agents can stay in iTerm2 instead (`"terminal": "iterm"`). See
  `artalde/docs/terminals.md`; building needs Zig 0.16 and the
  `artalde/vendor/ghostty` submodule.

- **Agents** — every Claude Code session reports through its hooks
  (`artalde hooks` prints the entries; `--install` merges them). Status is
  `blocked` (permission or question) > `done` (finished, unseen) > `working` >
  `idle`. Sessions started before the hook still show up, found by process.
- **Spaces** — the repo and worktree each agent actually touches (from tool
  paths, not its cwd), with branch, dirt and divergence.
- **Repository radar** — local worktrees and agents alongside open and merged
  PRs, check/review health, deployments and releases. PRs and repository
  metadata paint first; delivery activity loads separately.
- **Command palette** — Ctrl+K or Cmd+K searches agents, repositories, PRs,
  commands, layouts and themes. Prefix with `>`, `@` or `#` to scope the
  search; enter `owner/repo`, `owner/repo#123`, or `#123` in a repository to
  jump directly.
- **zaindu** — the inbox proposals, tickable from the window; open PRs via `gh`.
- **Act** — focus the agent's terminal tab, type a reply or a key into it,
  start a new agent in a worktree (`zz -c repo@branch claude`), macOS
  notifications when an agent blocks while you are elsewhere.
- **Dynamic UI** — the window is files in the wiki, `<wiki>/artalde/`:
  `layouts/*.json` (rows, columns, tabbed stacks, blocks), `themes/*.json`,
  `keys.json`, and `mods/*.rhai`, Rhai scripts that return UI trees from live
  data. Edits apply within a second; a broken file keeps its last good version
  and explains itself in `<file>.error`. `artalde check` validates headless.

```bash
artalde              # open the window (Ctrl/⌘K palette, ⌘J next agent that needs you)
artalde hooks        # the Claude Code hook entries
artalde check        # load the config, render every mod against live data
```

Code: `artalde/` (its own Cargo workspace, so egui never touches the other
tools). Agent guide for the UI files: `template/artalde/AGENTS.md`.

## The engineering library

Every wiki starts with a curated shelf in `template/concepts/`, hub
`library.md`. Each page cites an immutable copy of its source in `template/sources/`.

| Shelf | Pages |
|---|---|
| Stack | `stack-defaults` — sutegi + SvelteKit + osagai, when to deviate · `projects/sutegi`, `projects/osagai` |
| Language practice | `rust-style-guide` · `rust-type-driven-design` · `rust-resource-and-concurrency-patterns` · `ts-architecture` · `typescript-domain-patterns` · `typescript-async-and-resilience-patterns` · `rust-on-workers` |
| Measuring and checks | `verification-patterns` · `architecture-fitness-and-quality-gates` · `eval-methods` · `autoresearch` |
| Working with agents | `augmented-coding-patterns` · `graph-engineering` · `llm-wiki` |
| Choosing work | `inventing-work` |
| Systems design | `query-system-design` |

The template `CLAUDE.md` points every session at the right page: Rust edits →
the style guide, API/state design → the pattern guides, new apps → stack defaults,
and code review → `/quality-gate`. On upgrade, `install.sh` adds any
library page the wiki is missing and never overwrites one you already have.

## Linear (opt-in)

Off by default — without the key nothing ever calls Linear. To turn it on,
install the [`linear` CLI](https://github.com/schpet/linear-cli), authenticate
it (`linear auth login`, or `$LINEAR_API_KEY`), and add to the wiki's `os.yaml`:

```yaml
linear:
  teams: [APP]     # optional: team keys to recognise in PR titles and branches
```

`sare build` then projects the issues assigned to you (with their Linear
project and cycle) into the graph and `work.md` gains a **Tickets** section;
`zaindu poll` watches the same issues and proposes work when one is assigned,
finishes, is unblocked, is raised to high/urgent or gets a comment — and a
merged PR names the ticket it implements and the state Linear still has it in.
A project page claims a Linear project with `TRACKS lin:project:<id>`; the id is
in `sare hop <issue>`.

The 5-minute timer runs outside your shell, so it does not see variables your
shell profile exports. `scripts/zaindu-poll.sh` sources
`~/.config/thinking-os/env` when that file exists — put `export LINEAR_API_KEY=…`
there (mode 600) if the CLI is not logged in on its own. The details are in
`skills/graph/references/linear.md`.

## Agent workflow and shortcuts

`AGENTS.md` / `CLAUDE.md` route coding agents to the relevant workspace and
references. `make help` lists build, lint, test and check targets for both Rust
workspaces. Run `make artalde` to build and open the dashboard from this
checkout. From a wiki, the same target launches the installed binary with that
wiki's configuration. The CLI also accepts `artalde .` from a wiki or one of
its subdirectories. New and upgraded wikis get a Makefile (an existing one is preserved).

`sare today` collects bounded JSON briefing inputs offline: generated view
excerpts, active project metadata, deduplicated git probes, pending proposals,
recent log entries and the last briefing. `sare today --refresh` rebuilds first;
refresh output goes to stderr so stdout remains JSON. The `/today` skill owns
ranking and writing, and reads only shortlisted project bodies.

GitHub Issues work with your existing `gh` authentication and `me.github`.
Assigned open and recently closed issues flow through the graph, work view,
watcher/inbox, briefing collector, ikusi board and artalde ticket links. No
remote writes happen while polling. Set `github_issues: false` to opt out.
See `skills/graph/references/integrations.md` for priority labels, identity,
partial-read behavior and the additive ontology update for existing wikis.

## Developing Ikusi

The local viewer is a Svelte 5 app built with Osagai in `tools/ikusi-web`.
From that directory, run `pnpm install --frozen-lockfile`, `pnpm check`,
and `pnpm build`. Commit the resulting assets in `tools/src/ikusi/web` with
their source. Cargo embeds them so an installed viewer works offline.

Run `cargo run --manifest-path tools/Cargo.toml --bin ikusi -- serve` from the
repository root to serve the compiled viewer. For live UI editing, keep that
server on port 4747 and run `pnpm dev` from `tools/ikusi-web`. After building,
run `pnpm exec playwright install chromium --only-shell` and `pnpm test` for
the browser regression suite.

## Tests

```bash
cargo test --manifest-path tools/Cargo.toml
bash tools/e2e-zaindu.sh      # offline event chains (GitHub, Linear), then a live GitHub leg
packaging/test-install.sh    # offline binary install, update, and rollback checks
```

Maintainers publish the Linux and macOS bundles by pushing a tag matching
`VERSION`; see `RELEASING.md`.

The PR quality gate also runs strict Clippy and `cargo machete`:

```bash
cargo clippy --manifest-path tools/Cargo.toml --workspace --all-targets --all-features -- -D warnings
cargo machete tools
```

`/quality-gate` applies the engineering library's risk-specific checks to other
repositories and records exact-commit evidence when the wiki supports it.

## Left out, compared to the original

- **The frozen Python oracle** and its byte-identical differential test.
- **Personal tooling** — go-live watchdogs, notification hooks, machine-specific
  paths. zaindu sends no notifications.
