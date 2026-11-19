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
