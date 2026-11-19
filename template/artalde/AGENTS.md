# artalde — guide for agents

artalde is the thinking-os control plane for the coding agents on this
machine (engine: `thinking-os-core/artalde`). This folder of the wiki *is*
its UI: edit a file and the window follows within a second. Check your
edit with `artalde check`; it loads this folder, renders every mod against
live data, and exits non-zero on any error. A file that fails keeps its last
good version and the reason lands next to it in `<file>.error`.

## Files

| file | what |
|---|---|
| `settings.json` | appearance (`system`/`light`/`dark`), `theme` (dark) + `light_theme`, default layout, `editor`, notifications, spawn command lines, font size, `stale_after_mins` (default 60, 0 = off), `close_terminal` (close the iTerm2 tab on close) |
| `keys.json` | `"cmd+shift+l": "layout.cycle"` — chord → command |
| `layouts/<name>.json` | window trees; `layout.cycle` / `layout.select:<name>` |
| `themes/<name>.json` | colour tokens; missing tokens inherit graphite |
| `mods/<name>.rhai` | panels written as Rhai scripts |

## Layouts

```json
{ "title": "Mine", "root": { "row": [
  { "block": "spaces", "size": { "px": 260 }, "min": 180, "max": 480, "resizable": true },
  { "column": [ { "block": "mod:pulse", "size": { "px": 44 } }, { "block": "agents" } ] },
  { "stack": [ { "block": "detail", "title": "Session" }, { "block": "log" } ], "size": { "weight": 0.6 } }
] } }
```

- Nodes: `row`, `column` (split), `stack` (tabs), `block` (a panel).
- Child props: `size` (`{"px": n}` or `{"weight": n}`, default weight 1), `min`, `max`, `resizable`, `hidden`.
- Block/stack props: `title`, `padding`, `background` (token or `#hex`), `border` (default true). Splits: `gap`.
- Built-in panels: `agents`, `spaces`, `inbox` (zaindu), `prs` (the pull-request inbox, compact), `detail` (selected session + reply box), `status`, `log`. Mods: `mod:<name>`.
- Layouts fill the first tab. Pull requests (`inbox.open`, Cmd+I), single PRs (`pr.open:<owner/repo#N>`, or paste a PR URL into Cmd+K) and agent sessions (`session.open`, double-click a card) open as tabs; Cmd+W closes, Ctrl+Tab cycles.

## Themes

Tokens: `background surface raised border text dim faint accent working blocked done idle unknown danger`, plus `"dark": bool`. `faint` is tertiary text (captions, counts, hints); left out, it is mixed from `dim` and `surface`. graphite and paper are azala's dark and light palettes.
Values are `#rrggbb` or `#rrggbbaa`.

## Mods

A mod defines `fn render(ctx)` and returns a tree. It never draws or touches
files: it describes, artalde draws, and clicks come back as actions.

```rhai
fn render(ctx) {
    let blocked = ctx.sessions.filter(|s| s.state == "blocked");
    col([
        heading(`${blocked.len()} waiting`),
        wrap(blocked.map(|s| card([text(s.name).bold(), text(s.message ?? "").color("dim")])
            .fill("blocked").on_click(cmd("agent.select", s.id)))),
        button(if ctx.state.big ?? false { "smaller" } else { "bigger" }, set("big", !(ctx.state.big ?? false))),
    ])
}
```

- Nodes: `text(s)` `heading(s)` `md(s)` (rendered markdown) `row([..])` `col([..])` `wrap([..])` `grid(n, [..])` `card([..])`
  `button(label, action)` `badge(s)` `dot()` `sep()` `space(px)` `bar(0.0..1.0)`. A bare string is text; an array is a column.
- Colours: theme tokens, tag names (`blue`, `pink`, …) or `#rrggbb`.
- Modifiers: `.color(token)` `.fill(token)` (tinted background for cards/badges) `.size(px)` `.bold()` `.mono()` `.tip(s)` `.on_click(action)`.
- Actions: `cmd("name")`, `cmd("name", arg)`, `set("key", value)` (merges into `ctx.state`, re-renders; state survives edits).
- Helpers: `ago(secs)` → `3m`, `clip(s, n)`.
- `ctx`: `sessions` (each: `id agent state name age index selected repo branch worktree cwd tty pid title ai_title handle tag label last_prompt activity message task pending_tasks last_said discovered resources[kind id title state url mentions]`),
  `name` is the label, else the conversation title (`ai_title`), else the first prompt; `tag` is a palette colour usable as a token (`red orange yellow green teal blue purple pink`),
  `spaces` (`repo root worktrees[path branch dirty ahead behind]`), `prs` (`repo number title url draft updated`),
  `inbox` (`time kind target links detail url done`), `counts.{blocked,done,working,idle,unknown}`, `now`, `selected`, `filter`, `theme`, `state`.
- `render` reruns only when data, selection, theme or the mod's state change. Budget: 2M operations and 4000 nodes per render.

## Agent lifecycle

One opinionated flow, the same in the list, the Session panel, the palette and keys:

- **Start** (⌘N): prompt first. *Existing folder* or *New worktree*; the branch is derived from the prompt (`THU-4821-fix-csrf-timing`) until you edit it. The agent shows under **Starting** until its process appears, then is selected.
- **Lanes**: Needs you (blocked or finished unseen) → Working → Idle → **Stale** (idle longer than `stale_after_mins`).
- **Stop turn** (`.` / ⌘.): Esc in its terminal; the session stays.
- **Wrap up**: a finished Claude session gets `/wrap` typed in.
- **Close** (`x` / ⌘⌫): TERM, KILL after 4 s, then its iTerm2 tab closes. Asks first only when the agent is working or blocked. The pid is checked to still be that agent before any signal.
- **Close all stale** (⇧⌘⌫, or the button on the Stale lane): one confirmation for the lot.
- **Reopen** (⇧⌘T, or Recently closed): `claude --resume <id>` in a new tab. Closed sessions live in `<state dir>/closed.json` (last 30); `pid-*` and Codex sessions cannot resume.

## Commands

`artalde commands` lists them. Common: `agent.next`, `agent.select:<id|n>`, `session.focus[:<id>]`,
`session.ack[:<id>]`, `session.tag:<colour|none>` (or `<id>:<colour>`), `session.label:<text>`, `session.send:<text>`, `space.filter:<repo>`, `spawn.open`, `session.stop[:<id>]`, `session.close[:<id>]`, `session.sweep`, `session.reopen[:<id>]`, `session.wrap[:<id>]`, `layout.select:<name>`,
`layout.cycle`, `theme.select:<name>`, `theme.cycle`, `url.open:<url>`, `path.open:<path>`, `palette.open`, `refresh`,
`settings.open` (⌘,), `appearance.toggle` (⇧⌘D), `appearance.set:<system|light|dark>`, `path.edit:<path>`,
`pr.review` (⇧⌘R: agent-assisted review; proposals wait for your decision, nothing posts until Submit), `repo.open:<owner/repo>` (PRs, deployments per environment, releases),
`inbox.open`, `pr.open:<owner/repo#N>`, `session.open[:<id>]`, `tab.close`, `tab.next`, `tab.prev`, `tab.select:<n>`.
