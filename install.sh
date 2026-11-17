#!/bin/bash
# Install or upgrade thinking-os on this machine. Idempotent: re-run to upgrade.
#
#   ./install.sh [--wiki DIR] [--no-timer] [--skills-dir DIR]
#
# 1. checks git, gh (authenticated) and cargo
# 2. cargo-installs sare, zaindu, ikusi into ~/.local/bin
# 3. creates the wiki from template/ if DIR has none (git init, os.yaml, first
#    commit); on an existing wiki refreshes scripts/, adds library pages it is
#    missing, and reports — never applies — template changes to CLAUDE.md and
#    graph/ontology.yaml
# 4. symlinks each skills/<name> into the skills dir
# 5. installs + enables the 5-minute zaindu timer (systemd user / launchd)
# 6. seeds zaindu, builds the graph, prints a summary
#
# Never deletes or overwrites a wiki page.
set -euo pipefail

CORE="$(dirname "$(realpath "$0")")"
WIKI="${THINKING_OS_ROOT:-$HOME/thinking-os}"
SKILLS="$HOME/.claude/skills"
TIMER=1
BIN="$HOME/.local/bin"

while [ $# -gt 0 ]; do
  case "$1" in
    --wiki) WIKI="$2"; shift 2 ;;
    --skills-dir) SKILLS="$2"; shift 2 ;;
    --no-timer) TIMER=0; shift ;;
    -h|--help) sed -n '2,16p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "install.sh: unknown flag $1" >&2; exit 2 ;;
  esac
done
WIKI="$(realpath -m "$WIKI" 2>/dev/null || python3 -c 'import os,sys; print(os.path.abspath(os.path.expanduser(sys.argv[1])))' "$WIKI")"

step() { printf '\n\033[1m== %s\033[0m\n' "$1"; }
note() { printf '   %s\n' "$1"; }
die()  { printf '\033[31minstall.sh: %s\033[0m\n' "$1" >&2; exit 1; }
SUMMARY=()

PATH_BEFORE="$PATH"
export PATH="$BIN:$HOME/.cargo/bin:$PATH"

# ----------------------------------------------------------------- preflight
step "preflight"
for tool in git gh cargo; do
  command -v "$tool" >/dev/null || die "$tool not found — see skills/setup/SKILL.md"
done
gh auth status >/dev/null 2>&1 || die "gh is not authenticated — run: gh auth login"
LOGIN="$(gh api user -q .login)"
RUSTC="$(rustc --version | awk '{print $2}')"
case "$(printf '%s\n1.85.0\n' "$RUSTC" | sort -V | head -1)" in
  1.85.0) ;; *) die "rust $RUSTC is older than 1.85 (edition 2024) — run: rustup update stable" ;;
esac
note "gh login: $LOGIN · rust $RUSTC · $(uname -s)"

# ----------------------------------------------------------------- binaries
step "binaries → $BIN"
cargo install --quiet --path "$CORE/tools" --root "$HOME/.local" --force
for b in sare zaindu ikusi; do
  [ -x "$BIN/$b" ] || die "$b did not install"
done
note "sare zaindu ikusi installed"
SUMMARY+=("binaries     ok   $BIN/{sare,zaindu,ikusi}")

# ----------------------------------------------------------------- wiki
step "wiki → $WIKI"
if [ -f "$WIKI/graph/ontology.yaml" ]; then
  note "existing wiki: pages untouched"
  mkdir -p "$WIKI/scripts"
  cp "$CORE/template/scripts/"*.sh "$WIKI/scripts/"
  chmod +x "$WIKI/scripts/"*.sh
  note "scripts/ refreshed from the core"
  added=0
  for page in "$CORE"/template/{concepts,sources}/*.md "$CORE"/template/{tasks,attempts,tests}/_example.md \
              "$CORE"/template/projects/{sutegi,osagai}.md \
              "$CORE"/template/decisions/2026-11-17-default-stack-sutegi-osagai.md; do
    rel="${page#"$CORE/template/"}"
    [ -e "$WIKI/$rel" ] && continue
    mkdir -p "$(dirname "$WIKI/$rel")"
    cp "$page" "$WIKI/$rel"
    added=$((added + 1))
  done
  [ "$added" = 0 ] || note "library: $added missing page(s) added — commit them: git -C $WIKI add concepts sources projects"
  for f in CLAUDE.md graph/ontology.yaml .gitignore .gitattributes; do
    if [ -f "$WIKI/$f" ] && ! cmp -s "$CORE/template/$f" "$WIKI/$f"; then
      note "differs from template: $f — review with: diff $CORE/template/$f $WIKI/$f"
    fi
  done
  [ -f "$WIKI/os.yaml" ] || printf 'me:\n  github: %s\n' "$LOGIN" > "$WIKI/os.yaml"
  SUMMARY+=("wiki         ok   upgraded in place: $WIKI")
else
  [ -e "$WIKI" ] && [ -n "$(ls -A "$WIKI" 2>/dev/null)" ] && die "$WIKI exists and is not a wiki — pick another --wiki"
  mkdir -p "$WIKI"
  cp -R "$CORE/template/." "$WIKI/"
  chmod +x "$WIKI/scripts/"*.sh
  printf 'me:\n  github: %s\n' "$LOGIN" > "$WIKI/os.yaml"
  sed -i.bak "s/^## \[YYYY-MM-DD\] bootstrap/## [$(date +%F)] bootstrap/" "$WIKI/log.md" && rm -f "$WIKI/log.md.bak"
  git -C "$WIKI" init -q -b main
  git -C "$WIKI" add -A
  git -C "$WIKI" commit -q -m "bootstrap: thinking-os wiki from thinking-os-core template"
  note "created from template, first commit made"
  SUMMARY+=("wiki         ok   created: $WIKI")
fi

# ----------------------------------------------------------------- skills
step "skills → $SKILLS"
mkdir -p "$SKILLS"
linked=0
for dir in "$CORE"/skills/*/; do
  name="$(basename "$dir")"
  target="$SKILLS/$name"
  if [ -L "$target" ] || [ ! -e "$target" ]; then
    ln -sfn "${dir%/}" "$target"
    linked=$((linked + 1))
  else
    note "skipped $name: $target exists and is not a symlink"
  fi
done
note "$linked skills linked"
SUMMARY+=("skills       ok   $linked linked into $SKILLS")

# ----------------------------------------------------------------- timer
step "timer"
render() { sed "s#@WIKI@#$WIKI#g" "$1" > "$2"; }
if [ "$TIMER" = 0 ]; then
  note "skipped (--no-timer)"
  SUMMARY+=("timer        --   skipped")
elif [ "$(uname -s)" = "Darwin" ]; then
  plist="$HOME/Library/LaunchAgents/com.thinking-os.zaindu.plist"
  mkdir -p "$(dirname "$plist")"
  render "$CORE/deploy/launchd/com.thinking-os.zaindu.plist" "$plist"
  launchctl unload "$plist" 2>/dev/null || true
  launchctl load "$plist"
  SUMMARY+=("timer        ok   launchd com.thinking-os.zaindu, every 5 min")
elif systemctl --user show-environment >/dev/null 2>&1; then
  units="$HOME/.config/systemd/user"
  mkdir -p "$units"
  for u in thinking-os-zaindu.service thinking-os-zaindu.timer; do
    render "$CORE/deploy/systemd/$u" "$units/$u"
  done
  systemctl --user daemon-reload
  systemctl --user enable --now thinking-os-zaindu.timer >/dev/null
  if [ "$(loginctl show-user "$USER" -p Linger --value 2>/dev/null)" != "yes" ]; then
    note "linger is off: the timer only runs while you are logged in."
    note "for a headless box: sudo loginctl enable-linger $USER"
  fi
  SUMMARY+=("timer        ok   systemd thinking-os-zaindu.timer, every 5 min")
else
  note "no systemd user session — poll by cron instead:"
  note "*/5 * * * * $WIKI/scripts/zaindu-poll.sh"
  SUMMARY+=("timer        !!   no systemd user session; use cron")
fi

# ----------------------------------------------------------------- first run
step "first run"
export THINKING_OS_ROOT="$WIKI"
# seed only the first time: on an upgrade a seed would swallow real transitions
if [ -f "$WIKI/scripts/.gh-watch-state.json" ]; then
  zaindu poll --root "$WIKI" | tail -1 || note "zaindu poll failed — see above"
else
  zaindu poll --root "$WIKI" --seed | tail -1 || note "zaindu seed failed — see above"
fi
sare build >/dev/null 2>&1 || sare build pages >/dev/null
note "$(sare fuse --root "$WIKI" 2>/dev/null | head -1)"
sare serve --root "$WIKI" >/dev/null
rm -f "$WIKI/scripts/.autocommit-stamp"
"$WIKI/scripts/autocommit.sh" && note "$(git -C "$WIKI" log -1 --format=%s)"
SUMMARY+=("graph        ok   $(wc -l < "$WIKI/graph/nodes.tsv" | tr -d ' ') node rows, views in graph/views/")

step "summary"
printf '   %s\n' "${SUMMARY[@]}"
if ! case ":$PATH_BEFORE:" in *":$BIN:"*) true ;; *) false ;; esac; then
  note "add to your shell rc: export PATH=\"\$HOME/.local/bin:\$PATH\""
fi
note "next: cd $WIKI && claude   then try /today, /ingest, /graph"
