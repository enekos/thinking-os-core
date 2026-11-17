---
name: setup
description: Install or upgrade thinking-os on this machine — preflight the toolchain (Rust ≥1.85, gh authed, git, systemd user timers on Linux), run the core repo's install.sh (builds sare/zaindu/ikusi, creates the wiki from the template, links the skills, enables the 5-minute zaindu timer), then verify every piece. Triggers on "/setup", "install thinking-os", "set up the wiki on this machine", "upgrade thinking-os".
---

# /setup

Install thinking-os from the **thinking-os-core** repo onto this machine, or
upgrade an existing install. `install.sh` does the work; this skill checks the
ground first and proves the result after. Never deletes or overwrites a wiki.

## 1. Locate the core repo

This skill is normally a symlink `~/.claude/skills/setup → <core>/skills/setup`:

```bash
CORE=$(cd "$(dirname "$(readlink -f ~/.claude/skills/setup/SKILL.md)")/../.." 2>/dev/null && pwd)
[ -x "$CORE/install.sh" ] && echo "$CORE" || echo "not found"
```

On a first install the skill is not linked yet — the user is running it from a
fresh clone. Check the current directory and `~/src/thinking-os-core`,
`~/thinking-os-core`; otherwise ask for the path, or offer:

```bash
gh repo clone enekos/thinking-os-core ~/src/thinking-os-core
```

## 2. Preflight — one batched call, report, then fix

```bash
uname -sm
command -v git && git --version
command -v gh && gh --version | head -1 && gh auth status 2>&1 | head -3
command -v cargo && rustc --version
command -v systemctl && systemctl --user is-system-running 2>&1
loginctl show-user "$USER" -p Linger 2>/dev/null
echo "$PATH" | tr ':' '\n' | grep -x "$HOME/.local/bin" || echo "~/.local/bin NOT on PATH"
```

Requirements:

| Need | Check | If missing |
|---|---|---|
| git | `git --version` | distro package (`apt install git`, `dnf install git`, `pacman -S git`) |
| gh, authenticated | `gh auth status` | install gh, then the user runs `! gh auth login` |
| Rust ≥ 1.85 (edition 2024) | `rustc --version` | `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \| sh -s -- -y`, then `. ~/.cargo/env`; old rustup → `rustup update stable` |
| systemd user session (Linux) | `systemctl --user` answers | without it, install with `--no-timer` and poll by cron instead |
| linger (Linux, headless) | `Linger=yes` | `sudo loginctl enable-linger "$USER"` — otherwise the timer only runs while logged in |
| `~/.local/bin` on PATH | the grep above | add `export PATH="$HOME/.local/bin:$PATH"` to the shell rc |

`gh auth login` is interactive — ask the user to run it with `!`, don't attempt
it yourself. Anything needing `sudo` (packages, linger): show the command and
ask.

## 3. Choose flags, then run

Ask only what isn't obvious:

- **Wiki location** — default `~/thinking-os`. If a wiki already exists there
  (has `graph/ontology.yaml`), install.sh upgrades in place and never touches
  its pages.
- **Timer** — on by default. `--no-timer` for a machine that shouldn't poll GitHub.
- **Skills dir** — default `~/.claude/skills`.

```bash
"$CORE/install.sh" [--wiki DIR] [--no-timer] [--skills-dir DIR]
```

It: checks git/gh/cargo, `cargo install`s sare/zaindu/ikusi into `~/.local/bin`,
creates the wiki from `template/` if absent (git init, writes `os.yaml` with
`me.github` from `gh api user`, first commit), symlinks each `skills/<name>`
into the skills dir, installs and enables the timer, runs `zaindu poll --seed`
and `sare build`, and prints a summary. Idempotent.

The first build fetches the `sutegi` git dependency — needs network. Expect a
few minutes on a cold cargo cache.

## 4. Verify — don't trust the summary, check

```bash
W="${THINKING_OS_ROOT:-$HOME/thinking-os}"     # or the --wiki you chose
for b in sare zaindu ikusi; do printf '%-7s ' $b; command -v $b || echo MISSING; done
cat "$W/os.yaml"
ls -l ~/.claude/skills | grep thinking-os-core
systemctl --user list-timers --all | grep thinking-os     # Linux
launchctl list | grep thinking-os                         # macOS
zaindu status
sare build pages && head -20 "$W/index.md"
ikusi status || (ikusi serve --port 4747 & sleep 1; ikusi status)
git -C "$W" log --oneline -3
```

Pass = all three binaries resolve, `os.yaml` names the right GitHub login, every
skill is a symlink into the core repo, the timer is listed with a next run,
`zaindu status` shows a watched set, `sare build pages` exits 0 and `index.md`
lists the example pages, `ikusi status` answers.

Report one table: piece → ok / fixed / failed, with the failing output.

## 5. Optional — the default stack's repos

The wiki ships an engineering library (`concepts/library.md`) whose
`concepts/stack-defaults.md` points new apps at **sutegi** (Rust web framework) and
**osagai** (Svelte UI kit). Both are consumed from crates.io / npm, so cloning
is only needed to contribute or read their docs locally. Offer it; don't do it
unasked:

```bash
gh repo clone enekos/sutegi ~/src/sutegi
gh repo clone enekos/osagai ~/src/osagai
```

Then set `path:` in `projects/sutegi.md` and `projects/osagai.md` and run
`sare build` — health stops listing them as "no reachable repo" and their PRs
join `work.md`.

## Troubleshooting

| Symptom | Cause | Fix |
|---|---|---|
| `gh: not logged in` / zaindu polls 0 PRs | gh not authenticated | `! gh auth login`, rerun install.sh |
| `cargo: command not found` | no Rust toolchain | rustup one-liner above, `. ~/.cargo/env` |
| `edition 2024 is unstable` / feature errors | Rust < 1.85 | `rustup update stable` |
| `sare: command not found` after install | `~/.local/bin` not on PATH | add it to the shell rc, open a new shell |
| `failed to fetch … sutegi` | no network / proxy on first build | retry with network; cargo caches it afterwards |
| timer listed but never fires | no linger, no login session | `sudo loginctl enable-linger "$USER"` |
| `Failed to connect to bus` | no systemd user session (container, WSL1) | `--no-timer`; cron `*/5 * * * * <wiki>/scripts/zaindu-poll.sh` |
| `ikusi` can't open a browser | headless box | `ikusi serve`, then `ssh -L 4747:127.0.0.1:4747 <host>` |
| `os.yaml` has the wrong login | gh authed as another account | `gh auth switch`, edit `me.github`, rerun |

Logs: `<wiki>/scripts/zaindu-poll.log`,
`journalctl --user -u thinking-os-zaindu.service -n 50`.

## Upgrade

```bash
git -C "$CORE" pull --ff-only && "$CORE/install.sh" [same flags]
```

Rebuilds the binaries and re-links skills. Library pages the wiki is missing
(new in the core's `template/concepts`, `sources`, `projects/{sutegi,osagai}`)
are added; existing pages are never touched;
template changes to `CLAUDE.md` or `ontology.yaml` are reported, not applied —
diff `"$CORE/template/<file>"` against the wiki's copy and merge by hand.

## Uninstall

Removes the engine, never the wiki:

```bash
systemctl --user disable --now thinking-os-zaindu.timer          # Linux
rm -f ~/.config/systemd/user/thinking-os-zaindu.{service,timer}
systemctl --user daemon-reload
for s in "$CORE"/skills/*/; do rm -f ~/.claude/skills/"$(basename "$s")"; done
cargo uninstall --root ~/.local thinking-os-tools
```

On macOS, `launchctl unload` the plist from `~/Library/LaunchAgents/` instead
of the systemd lines. The wiki directory stays; delete it yourself if you mean it.
