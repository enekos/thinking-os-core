---
name: setup
description: Install or upgrade thinking-os-core on this machine using published binaries — preflight git, authenticated gh, curl, and timers; create or update the wiki, link skills, and verify the installation. No Rust toolchain is needed. Triggers on "/setup", "install thinking-os", "set up the wiki on this machine", "upgrade thinking-os".
---

# /setup

Install **thinking-os-core** from a published release, or upgrade an existing
installation. The normal path downloads binaries and supporting files; it does
not clone the engine repository or invoke Cargo. Never delete or overwrite a
personal wiki page.

## 1. Preflight

Run one batched check and report missing pieces before installing:

```bash
uname -sm
command -v git && git --version
command -v gh && gh --version | head -1 && gh auth status 2>&1 | head -3
command -v curl && command -v tar
command -v systemctl && systemctl --user is-system-running 2>&1
loginctl show-user "$USER" -p Linger 2>/dev/null
echo "$PATH" | tr ':' '\n' | grep -x "$HOME/.local/bin" || echo "~/.local/bin NOT on PATH"
```

Requirements:

| Need | Check | If missing |
|---|---|---|
| git | `git --version` | Install the distro or Xcode command-line tools package |
| gh, authenticated | `gh auth status` | Install gh, then ask the user to run `! gh auth login` |
| curl and tar | `command -v curl tar` | Install the standard system packages |
| Supported target | `uname -sm` | Linux x86_64 or macOS Apple Silicon; other targets need a source build |
| systemd user session (Linux) | `systemctl --user` answers | Install with `--no-timer` and poll by cron instead |
| linger (Linux, headless) | `Linger=yes` | Ask before `sudo loginctl enable-linger "$USER"` |
| `~/.local/bin` on PATH | The check above | Add `export PATH="$HOME/.local/bin:$PATH"` to the shell rc |

`gh auth login` is interactive — ask the user to run it with `!`; do not attempt
it yourself. Anything needing `sudo` needs an explicit user request.

## 2. Install or choose the source-build path

The default wiki is `~/thinking-os`. Ask only about choices that are not
obvious: `--wiki DIR`, `--no-timer`, `--no-artalde`, and `--skills-dir DIR`.

For a normal first install, run:

```bash
curl -fsSL https://github.com/enekos/thinking-os-core/releases/latest/download/install.sh | bash
```

To pass flags through the pipe:

```bash
curl -fsSL https://github.com/enekos/thinking-os-core/releases/latest/download/install.sh \
  | bash -s -- --wiki "$HOME/thinking-os" --no-timer
```

If the user explicitly wants to develop from a checkout, use
`./install.sh --from-source`; only that path needs the current Rust toolchain.
The source path still creates/upgrades the wiki and links the skills.

The installer detects the platform, downloads the complete release archive,
verifies `SHA256SUMS`, smoke-tests the binaries, and activates the release. It
stores the chosen options in `~/.config/thinking-os/install.conf` and installs
`thinking-os`, `sare`, `zaindu`, `ikusi`, and optionally `artalde` in
`~/.local/bin`.

## 3. Verify

Do not trust the summary alone:

```bash
W="${THINKING_OS_ROOT:-$HOME/thinking-os}"
thinking-os version
for b in sare zaindu ikusi; do printf '%-7s ' "$b"; command -v "$b" || echo MISSING; done
cat "$W/os.yaml"
systemctl --user list-timers --all | grep thinking-os     # Linux
launchctl list | grep thinking-os                         # macOS
zaindu status
sare build pages && head -20 "$W/index.md"
ikusi status || (ikusi serve --port 4747 & sleep 1; ikusi status)
git -C "$W" log --oneline -3
```

Pass = the release version and target are reported, all three core binaries
resolve, `os.yaml` names the right GitHub login, skills are linked into the
active release, the timer has a next run when enabled, and the graph commands
succeed. `artalde --version` should also work unless `--no-artalde` was chosen.

Report one table: piece → ok / fixed / failed, with failing output.

## Upgrade

For an installed release, simply run:

```bash
thinking-os update
```

`thinking-os update --check` only reports installed and latest versions. Updates
reuse the saved wiki, skills, timer, and artalde options; there is no `git pull`
or rebuild. A new archive is verified before the `current` symlink changes, so
a failed download or checksum leaves the previous release active.

Engine-managed skills and wiki scripts are refreshed. New template pages are
added, existing personal pages are untouched, and differences in `CLAUDE.md`
or `graph/ontology.yaml` are printed for manual review and merging.

## Optional — the default stack's repos

The wiki ships an engineering library whose `concepts/stack-defaults.md` points
new apps at **sutegi** and **osagai**. They are consumed from crates.io / npm;
cloning is only needed to contribute or read their docs locally. Offer this,
but do not do it unasked:

```bash
gh repo clone enekos/sutegi ~/src/sutegi
gh repo clone enekos/osagai ~/src/osagai
```

Then set `path:` in `projects/sutegi.md` and `projects/osagai.md` and run
`sare build`.

## Troubleshooting

| Symptom | Cause | Fix |
|---|---|---|
| `gh: not logged in` / zaindu polls 0 PRs | gh not authenticated | `! gh auth login`, then rerun the installer or poll |
| `no prebuilt release` | Unsupported OS/architecture | Use a supported target or `./install.sh --from-source` |
| `checksum mismatch` | Corrupt or replaced download | Retry; the active release was not changed |
| `sare: command not found` | `~/.local/bin` not on PATH | Add it to the shell rc and open a new shell |
| timer listed but never fires | No linger or login session | Ask before `sudo loginctl enable-linger "$USER"` |
| `Failed to connect to bus` | No systemd user session | Install with `--no-timer`; use cron for `scripts/zaindu-poll.sh` |
| `ikusi` cannot open a browser | Headless machine | `ikusi serve`, then SSH-forward port 4747 |
| `os.yaml` has the wrong login | gh authenticated as another account | `gh auth switch`, edit `me.github`, rerun |

Logs: `<wiki>/scripts/zaindu-poll.log`,
`journalctl --user -u thinking-os-zaindu.service -n 50`.

## Uninstall

Stop the timer and remove only the engine-managed links and release data. Keep
the wiki unless the user explicitly asks to delete it:

```bash
systemctl --user disable --now thinking-os-zaindu.timer          # Linux
rm -f ~/.config/systemd/user/thinking-os-zaindu.{service,timer}
systemctl --user daemon-reload
rm -f ~/.local/bin/{thinking-os,sare,zaindu,ikusi,artalde}
rm -rf ~/.local/share/thinking-os-core
```

On macOS, `launchctl unload ~/Library/LaunchAgents/com.thinking-os.zaindu.plist`
and remove that plist instead of the systemd lines. Remove skill symlinks only
when they point into `~/.local/share/thinking-os-core`; leave real user-owned
skill directories alone. The wiki directory stays; delete it yourself only if
that is explicitly intended.
