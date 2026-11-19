---
name: artalde
description: Operate and reshape artalde, the thinking-os agent control plane — a native window over every coding agent on the machine (blocked/done/working, worktrees, zaindu inbox, PRs). Use when asked to open it, wire its Claude Code hook, add or change a layout, theme, key or panel ("add a panel that shows…", "make the board compact by default"), write a Rhai mod, or debug why an agent shows the wrong state. Triggers on "/artalde", "agent dashboard", "control plane", "which agents need me".
---

# artalde

The window is data in the wiki: `<wiki>/artalde/` (`$ARTALDE_CONFIG_DIR`
overrides). The engine is `thinking-os-core/artalde/`. Read
`<wiki>/artalde/AGENTS.md` before editing anything in that folder — it has the
layout schema, theme tokens, the mod API and every command.

## Run and wire

```bash
artalde                    # open the window
artalde hooks              # print the Claude Code hook entries
artalde hooks --install    # merge them into ~/.claude/settings.json (backs it up first)
artalde snapshot | jq '.sessions[] | {state, name: .title, repo, tty}'
```

Ask before `--install`: it edits the user's global Claude Code settings.
Agents other than Claude report with
`artalde report --id <id> --state working|blocked|done|idle [--message M]`.

## Reshape the UI

1. Edit `layouts/*.json`, `themes/*.json`, `keys.json` or `mods/*.rhai` in
   `<wiki>/artalde/`. A running window follows within a second.
2. Run `artalde check`. It exits non-zero on any refused file or failing mod;
   the reason is also in `<file>.error`.
3. A new panel is a mod: `fn render(ctx)` returning `col([...])` etc. Prefer a
   mod over Rust changes; reach for the engine only for new data or commands.
4. Commit the folder with the wiki (`*.error` files are gitignored).

## Lifecycle

Start (⌘N, prompt-first, branch from the prompt) → Needs you / Working / Idle → Stale
(`stale_after_mins`) → Close (`x`: TERM → KILL, iTerm2 tab closed; asks only if working or
blocked) → Reopen (⇧⌘T: `claude --resume`). Stop a turn with `.`, close every stale agent with
⇧⌘⌫. Closing kills real processes — never run `session.close`/`session.sweep` for the user
without their say-so. Closed log: `<state dir>/closed.json`.

## Identify sessions

- Names come from Claude Code itself: `~/.claude/sessions/<pid>.json` (session
  id, short name, live `busy/idle/waiting` status — artalde shows state even
  without the hook) and the transcript's `ai-title` (the conversation title).
- Colour-tag a session (`session.tag:<colour>`, swatches in the Session panel
  or the palette); the tag also colours its iTerm2 tab. Labels override titles.
  Both live in `~/.local/state/artalde/tags.json`.
- Related PRs and tickets are mined from the transcript (GitHub PR URLs, Linear
  ids for the teams in `os.yaml`, the branch name) and resolved against
  `<wiki>/graph/raw/{watch,github,linear}.json` — no network calls.

## Pull requests (after adar)

- Cmd+I opens the inbox (review requested, mine, involved, recently merged)
  with adar's filter qualifiers (`repo:data ci:red review:changes @alice
  -is:bot size:<100 in:review "words"`). Rows open PR tabs.
- A PR tab has Conversation (rendered body, review threads with reply /
  resolve, timeline, comment composer with preview), Files (diff + file list)
  and Checks (by workflow, failed-step logs inline, re-run job / failed).
- Actions: approve, request changes, comment, review comment, merge (allowed
  methods, delete branch), auto-merge, update branch, ready / draft, close /
  reopen. Irreversible ones ask first; none retries. All go through `gh`.
- Headless: `artalde inbox`, `artalde pr <owner/repo#N>`, `artalde repo <owner/repo>` (what is
  deployed where), `artalde review <owner/repo#N> [--agent NAME]` (read-only agent review; its
  proposals land in the PR's Review tab for the user to accept or dismiss — never posted by itself).
- Review agents live in settings.json `review_agents` (`{dir}` holds pr.md + pr.diff, `{checkout}`
  the head-branch worktree if one exists); runs and their logs are under
  `~/.local/state/artalde/reviews/<owner>__<repo>__<n>/`.

## Debug state

- Wrong or missing state: `artalde snapshot`, then the record in
  `~/.local/state/artalde/sessions/<session id>.json`. `discovered: true`
  means the session never reported — the hook is not wired for it.
- All agents in one space: they touched no files outside their cwd yet; the
  space follows the last absolute path a tool used.
- Focus/reply needs iTerm2 (Terminal.app works for focus and spawn only).

## Change the engine

`thinking-os-core/artalde` is a Cargo workspace: `artalde-core` (model, hook
state machine, probes, config), `artalde-mods` (Rhai host), `artalde` (egui
app + CLI). Follow `concepts/rust-style-guide.md`, then:

```bash
cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
ARTALDE_LAYOUT=main ARTALDE_SCREENSHOT=/tmp/a.png artalde   # self-screenshot, then quits
```

Drive the real UI offscreen — no window, no stolen focus — with a script of
`size`, `wait`, `move`, `click`, `dclick`, `rclick`, `scroll`, `type`, `key`,
`cmd` and `shot` steps (format in `crates/artalde/src/drive.rs`). Every side
effect (terminals, processes, gh writes, review agents, zaindu ticks) is only
logged. Point it at scratch copies of the config and state:

```bash
cargo build --features headless
ARTALDE_CONFIG_DIR=$scratch/config ARTALDE_STATE_DIR=$scratch/state \
  ~/.cargo/shared-target/debug/artalde drive script.txt
```

UI rules the kit enforces: chrome labels are not selectable (a click on a row's
text reaches the row; `kit::selectable_text` opts reading surfaces back in);
controls are `kit::CONTROL` tall and painted at fixed sizes; `add_sized`
centres, so fixed-width captions use `kit::caption`.
