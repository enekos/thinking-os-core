#!/bin/bash
# Install a published thinking-os-core release, or build a source checkout with
# --from-source. Re-running configures in place; `thinking-os update` fetches.
set -euo pipefail

SOURCE_PATH="${BASH_SOURCE[0]:-}"
CORE=""
if [ -n "$SOURCE_PATH" ] && [ -f "$SOURCE_PATH" ]; then
  while [ -L "$SOURCE_PATH" ]; do
    SOURCE_DIR="$(cd -P "$(dirname "$SOURCE_PATH")" && pwd)"
    SOURCE_LINK="$(readlink "$SOURCE_PATH")"
    case "$SOURCE_LINK" in
      /*) SOURCE_PATH="$SOURCE_LINK" ;;
      *) SOURCE_PATH="$SOURCE_DIR/$SOURCE_LINK" ;;
    esac
  done
  CORE="$(cd -P "$(dirname "$SOURCE_PATH")" && pwd)"
fi

CONFIG_DIR="${THINKING_OS_CONFIG_DIR:-${XDG_CONFIG_HOME:-$HOME/.config}/thinking-os}"
CONFIG_FILE="$CONFIG_DIR/install.conf"
DATA_DIR="${THINKING_OS_DATA_DIR:-${XDG_DATA_HOME:-$HOME/.local/share}/thinking-os-core}"
BIN="${THINKING_OS_BIN_DIR:-$HOME/.local/bin}"
REPOSITORY="${THINKING_OS_RELEASE_REPOSITORY:-enekos/thinking-os-core}"
RELEASE_BASE_URL="${THINKING_OS_RELEASE_BASE_URL:-https://github.com/$REPOSITORY/releases/download}"

SAVED_WIKI=""
SAVED_SKILLS=""
SAVED_TIMER=1
SAVED_ARTALDE=1
if [ -r "$CONFIG_FILE" ]; then
  # Written by this installer with shell-escaped values.
  . "$CONFIG_FILE"
fi

WIKI="${THINKING_OS_ROOT:-${SAVED_WIKI:-$HOME/thinking-os}}"
SKILLS="${SAVED_SKILLS:-$HOME/.claude/skills}"
TIMER="$SAVED_TIMER"
ARTALDE="$SAVED_ARTALDE"
FROM_SOURCE=0
FROM_BUNDLE=0
UPDATE=0
REQUESTED_VERSION=""

usage() {
  cat <<'EOF'
usage: install.sh [--wiki DIR] [--skills-dir DIR] [--no-timer] [--no-artalde]
                  [--version X.Y.Z] [--from-source]

Without --from-source, downloads a published binary release. A source checkout
therefore does not need Rust for the default installation path. Options are
saved and reused by `thinking-os update`.
EOF
}

while [ $# -gt 0 ]; do
  case "$1" in
    --wiki) [ $# -ge 2 ] || { usage >&2; exit 2; }; WIKI="$2"; shift 2 ;;
    --skills-dir) [ $# -ge 2 ] || { usage >&2; exit 2; }; SKILLS="$2"; shift 2 ;;
    --no-timer) TIMER=0; shift ;;
    --no-artalde) ARTALDE=0; shift ;;
    --from-source) FROM_SOURCE=1; shift ;;
    --from-bundle) FROM_BUNDLE=1; shift ;;
    --update) UPDATE=1; shift ;;
    --version)
      [ $# -ge 2 ] || { usage >&2; exit 2; }
      REQUESTED_VERSION="${2#v}"
      shift 2
      ;;
    -h|--help) usage; exit 0 ;;
    *) printf 'install.sh: unknown flag %s\n' "$1" >&2; exit 2 ;;
  esac
done

if [ "$FROM_SOURCE" = 1 ] && { [ "$UPDATE" = 1 ] || [ "$FROM_BUNDLE" = 1 ]; }; then
  printf 'install.sh: --from-source cannot be combined with update/bundle modes\n' >&2
  exit 2
fi
case "$WIKI" in /*) ;; *) WIKI="$PWD/$WIKI" ;; esac
case "$SKILLS" in /*) ;; *) SKILLS="$PWD/$SKILLS" ;; esac
case "$BIN" in /*) ;; *) BIN="$PWD/$BIN" ;; esac

step() { printf '\n\033[1m== %s\033[0m\n' "$1"; }
note() { printf '   %s\n' "$1"; }
die() { printf '\033[31minstall.sh: %s\033[0m\n' "$1" >&2; exit 1; }
SUMMARY=()
WORK_DIR=""
cleanup() {
  if [ -n "$WORK_DIR" ] && [ -d "$WORK_DIR" ]; then
    rm -rf "$WORK_DIR"
  fi
}
trap cleanup EXIT

PATH_BEFORE="$PATH"
export PATH="$BIN:$HOME/.cargo/bin:/opt/homebrew/bin:/usr/local/bin:$PATH"

detect_target() {
  if [ -n "${THINKING_OS_TARGET:-}" ]; then
    printf '%s\n' "$THINKING_OS_TARGET"
    return
  fi
  case "$(uname -s)-$(uname -m)" in
    Linux-x86_64) printf '%s\n' x86_64-unknown-linux-gnu ;;
    Darwin-arm64|Darwin-aarch64) printf '%s\n' aarch64-apple-darwin ;;
    *) die "no prebuilt release for $(uname -sm); use --from-source on a source checkout" ;;
  esac
}
TARGET="$(detect_target)"
BUNDLE=0
if [ -n "$CORE" ] && [ -f "$CORE/VERSION" ] && [ -x "$CORE/bin/sare" ] && [ -x "$CORE/bin/zaindu" ] && [ -x "$CORE/bin/ikusi" ]; then
  BUNDLE=1
fi
[ "$FROM_BUNDLE" = 0 ] || [ "$BUNDLE" = 1 ] || die "--from-bundle requires an unpacked release"
[ "$FROM_SOURCE" = 0 ] || [ -n "$CORE" ] || die "--from-source requires a source checkout"

step "preflight"
for tool in git gh curl tar; do
  command -v "$tool" >/dev/null || die "$tool not found — install it and retry"
done
if [ "$FROM_SOURCE" = 1 ]; then
  for tool in cargo rustc; do
    command -v "$tool" >/dev/null || die "$tool not found — source builds need the current Rust toolchain"
  done
fi
gh auth status >/dev/null 2>&1 || die "gh is not authenticated — run: gh auth login"
LOGIN="$(gh api user -q .login)"
note "gh login: $LOGIN · $TARGET"

stop_timer() {
  [ "$TIMER" = 1 ] || return 0
  if [ "$(uname -s)" = Darwin ]; then
    launchctl unload "$HOME/Library/LaunchAgents/com.thinking-os.zaindu.plist" 2>/dev/null || true
  else
    systemctl --user stop thinking-os-zaindu.timer 2>/dev/null || true
  fi
}

save_config() {
  mkdir -p "$CONFIG_DIR"
  local temporary="$CONFIG_FILE.tmp.$$"
  {
    printf 'SAVED_WIKI=%q\n' "$WIKI"
    printf 'SAVED_SKILLS=%q\n' "$SKILLS"
    printf 'SAVED_TIMER=%q\n' "$TIMER"
    printf 'SAVED_ARTALDE=%q\n' "$ARTALDE"
  } > "$temporary"
  chmod 600 "$temporary"
  mv -f "$temporary" "$CONFIG_FILE"
}

sha256_file() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  else
    shasum -a 256 "$1" | awk '{print $1}'
  fi
}

latest_version() {
  if [ -n "${THINKING_OS_LATEST_VERSION:-}" ]; then
    printf '%s\n' "${THINKING_OS_LATEST_VERSION#v}"
  else
    gh api "repos/$REPOSITORY/releases/latest" --jq .tag_name | sed 's/^v//'
  fi
}

forward_to_bundle() {
  local bundle_root="$1"
  local arguments=(--from-bundle --wiki "$WIKI" --skills-dir "$SKILLS")
  [ "$TIMER" = 1 ] || arguments+=(--no-timer)
  [ "$ARTALDE" = 1 ] || arguments+=(--no-artalde)
  "$bundle_root/install.sh" "${arguments[@]}"
}

download_release() {
  local version="$REQUESTED_VERSION"
  [ -n "$version" ] || version="$(latest_version)"
  case "$version" in
    ''|*[!0-9A-Za-z.-]*) die "invalid release version: $version" ;;
  esac
  local tag="v$version"
  local package="thinking-os-core-$version-$TARGET"
  local archive="$package.tar.gz"
  local destination="$DATA_DIR/versions/$version-$TARGET"

  if [ -x "$destination/bin/sare" ] && [ -f "$destination/VERSION" ] && [ "$(cat "$destination/VERSION")" = "$version" ]; then
    note "release $tag already downloaded"
    forward_to_bundle "$destination"
    exit $?
  fi

  mkdir -p "$DATA_DIR/versions"
  WORK_DIR="$(mktemp -d "${TMPDIR:-/tmp}/thinking-os-release.XXXXXX")"
  local archive_path="$WORK_DIR/$archive"
  local checksums_path="$WORK_DIR/SHA256SUMS"
  step "download $tag ($TARGET)"
  curl -fL --retry 3 -o "$archive_path" "$RELEASE_BASE_URL/$tag/$archive"
  curl -fL --retry 3 -o "$checksums_path" "$RELEASE_BASE_URL/$tag/SHA256SUMS"
  local expected actual
  expected="$(awk -v asset="$archive" '$2 == asset {print $1}' "$checksums_path")"
  [ -n "$expected" ] || die "release checksum does not contain $archive"
  actual="$(sha256_file "$archive_path")"
  [ "$actual" = "$expected" ] || die "checksum mismatch for $archive; nothing was activated"

  tar -xzf "$archive_path" -C "$WORK_DIR"
  [ -x "$WORK_DIR/$package/install.sh" ] || die "release archive has no installer"
  [ -f "$WORK_DIR/$package/VERSION" ] || die "release archive has no version metadata"
  [ "$(cat "$WORK_DIR/$package/VERSION")" = "$version" ] || die "release archive version does not match $tag"
  [ -f "$WORK_DIR/$package/TARGET" ] || die "release archive has no target metadata"
  [ "$(cat "$WORK_DIR/$package/TARGET")" = "$TARGET" ] || die "release archive target does not match $TARGET"
  if [ -e "$destination" ]; then
    current_destination="$(readlink "$DATA_DIR/current" 2>/dev/null || true)"
    [ "$current_destination" != "$destination" ] || die "active release cache is incomplete; refusing to replace it"
    case "$destination" in "$DATA_DIR"/versions/*) rm -rf "$destination" ;; *) die "unsafe release destination" ;; esac
  fi
  mv "$WORK_DIR/$package" "$destination"
  forward_to_bundle "$destination"
  exit $?
}

if [ "$FROM_SOURCE" = 0 ] && { [ "$UPDATE" = 1 ] || [ "$BUNDLE" = 0 ]; }; then
  download_release
fi

step "binaries → $BIN"
stop_timer
mkdir -p "$BIN"
if [ "$FROM_SOURCE" = 1 ]; then
  cargo install --quiet --path "$CORE/tools" --root "$(dirname "$BIN")" --force
  for tool in sare zaindu ikusi; do
    [ -x "$BIN/$tool" ] || die "$tool did not install"
  done
  ln -sfn "$CORE/packaging/thinking-os.sh" "$BIN/thinking-os"
  note "sare zaindu ikusi installed from source"
  SUMMARY+=("binaries     ok   $BIN/{sare,zaindu,ikusi} (source)")
  if [ "$ARTALDE" = 1 ]; then
    if cargo install --quiet --path "$CORE/artalde/crates/artalde" --root "$(dirname "$BIN")" --force; then
      note "artalde installed — wire its hook with: artalde hooks"
      SUMMARY+=("artalde      ok   $BIN/artalde (hook: artalde hooks)")
    else
      note "artalde did not build — the rest of thinking-os is unaffected"
      SUMMARY+=("artalde      !!   build failed; re-run with --no-artalde to skip")
    fi
  else
    note "artalde skipped (--no-artalde)"
  fi
else
  [ -f "$CORE/TARGET" ] && [ "$(cat "$CORE/TARGET")" = "$TARGET" ] || die "bundle target mismatch"
  for tool in sare zaindu ikusi thinking-os; do
    [ -x "$CORE/bin/$tool" ] || die "release is missing $tool"
  done
  for tool in sare zaindu ikusi; do
    "$CORE/bin/$tool" --version >/dev/null || die "$tool failed its smoke test"
  done
  if [ "$ARTALDE" = 1 ]; then
    [ -x "$CORE/bin/artalde" ] || die "release is missing artalde (use --no-artalde to skip it)"
    "$CORE/bin/artalde" --version >/dev/null || die "artalde failed its smoke test"
  fi
  mkdir -p "$DATA_DIR"
  ln -sfn "$CORE" "$DATA_DIR/current"
  for tool in sare zaindu ikusi thinking-os; do
    ln -sfn "$DATA_DIR/current/bin/$tool" "$BIN/$tool"
  done
  if [ "$ARTALDE" = 1 ]; then
    ln -sfn "$DATA_DIR/current/bin/artalde" "$BIN/artalde"
  elif [ -L "$BIN/artalde" ] && [ "$(readlink "$BIN/artalde")" = "$DATA_DIR/current/bin/artalde" ]; then
    rm -f "$BIN/artalde"
  fi
  note "release v$(cat "$CORE/VERSION") activated"
  SUMMARY+=("binaries     ok   $BIN/{sare,zaindu,ikusi} (prebuilt)")
  if [ "$ARTALDE" = 1 ]; then
    SUMMARY+=("artalde      ok   $BIN/artalde (prebuilt)")
  else
    SUMMARY+=("artalde      --   skipped")
  fi
fi
save_config

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
    relative="${page#"$CORE/template/"}"
    [ -e "$WIKI/$relative" ] && continue
    mkdir -p "$(dirname "$WIKI/$relative")"
    cp "$page" "$WIKI/$relative"
    added=$((added + 1))
  done
  for file in "$CORE"/template/artalde/{*.json,*.md} "$CORE"/template/artalde/*/*; do
    relative="${file#"$CORE/template/"}"
    [ -e "$WIKI/$relative" ] && continue
    mkdir -p "$(dirname "$WIKI/$relative")"
    cp "$file" "$WIKI/$relative"
    added=$((added + 1))
  done
  [ "$added" = 0 ] || note "library: $added missing page(s) added — commit them: git -C $WIKI add concepts sources projects artalde"
  for file in CLAUDE.md graph/ontology.yaml .gitignore .gitattributes; do
    if [ -f "$WIKI/$file" ] && [ -f "$CORE/template/$file" ] && ! cmp -s "$CORE/template/$file" "$WIKI/$file"; then
      note "differs from template: $file — review with: diff $CORE/template/$file $WIKI/$file"
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

step "skills → $SKILLS"
mkdir -p "$SKILLS"
linked=0
skipped=()
SKILL_LINK_ROOT="$CORE/skills"
if [ "$FROM_SOURCE" = 0 ]; then
  SKILL_LINK_ROOT="$DATA_DIR/current/skills"
fi
for directory in "$CORE"/skills/*/; do
  name="$(basename "$directory")"
  target="$SKILLS/$name"
  if [ -L "$target" ] || [ ! -e "$target" ]; then
    ln -sfn "$SKILL_LINK_ROOT/$name" "$SKILLS/$name"
    linked=$((linked + 1))
  else
    note "skipped $name: $target exists and is not a symlink"
    skipped+=("$name")
  fi
done
note "$linked skills linked"
if [ ${#skipped[@]} -eq 0 ]; then
  SUMMARY+=("skills       ok   $linked linked into $SKILLS")
else
  SUMMARY+=("skills       !!   $linked linked; not replaced (your own copy wins): ${skipped[*]}")
fi

step "first run"
export THINKING_OS_ROOT="$WIKI"
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
  for other in "$HOME"/Library/LaunchAgents/*.plist; do
    [ "$other" = "$plist" ] && continue
    if grep -q -F "$WIKI/" "$other" 2>/dev/null; then
      note "also points at this wiki: $other"
      note "if it is an old poller, remove it: launchctl unload $other"
      SUMMARY+=("timer        !!   another launchd agent uses this wiki: $(basename "$other")")
    fi
  done
elif systemctl --user show-environment >/dev/null 2>&1; then
  units="$HOME/.config/systemd/user"
  mkdir -p "$units"
  for unit in thinking-os-zaindu.service thinking-os-zaindu.timer; do
    render "$CORE/deploy/systemd/$unit" "$units/$unit"
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

step "summary"
printf '   %s\n' "${SUMMARY[@]}"
if ! case ":$PATH_BEFORE:" in *":$BIN:"*) true ;; *) false ;; esac; then
  note "add to your shell rc: export PATH=\"$BIN:\$PATH\""
fi
note "next: cd $WIKI && claude   then try /today, /ingest, /graph"
