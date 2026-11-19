#!/bin/bash
# Offline integration test for the release installer and updater.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
TEMP_DIR="$(mktemp -d "${TMPDIR:-/tmp}/thinking-os-install-test.XXXXXX")"
trap 'rm -rf "$TEMP_DIR"' EXIT

case "$(uname -s)-$(uname -m)" in
  Linux-x86_64) TARGET=x86_64-unknown-linux-gnu ;;
  Darwin-arm64|Darwin-aarch64) TARGET=aarch64-apple-darwin ;;
  *) printf 'test-install.sh: unsupported test host %s\n' "$(uname -sm)" >&2; exit 1 ;;
esac

RELEASES="$TEMP_DIR/releases"
TEST_HOME="$TEMP_DIR/home"
FAKE_BIN="$TEMP_DIR/fake-bin"
mkdir -p "$RELEASES" "$TEST_HOME" "$FAKE_BIN"

cat > "$FAKE_BIN/gh" <<'EOF'
#!/bin/bash
set -euo pipefail
if [ "${1:-}" = auth ]; then exit 0; fi
if [ "${1:-}" = api ] && [ "${2:-}" = user ]; then printf 'test-user\n'; exit 0; fi
if [ "${1:-}" = api ]; then printf 'v%s\n' "${THINKING_OS_LATEST_VERSION#v}"; exit 0; fi
exit 2
EOF
cat > "$FAKE_BIN/cargo" <<EOF
#!/bin/bash
printf called > "$TEMP_DIR/cargo-called"
exit 99
EOF
cat > "$FAKE_BIN/rustc" <<EOF
#!/bin/bash
printf called > "$TEMP_DIR/rustc-called"
exit 99
EOF
chmod +x "$FAKE_BIN/"*

make_release() {
  local version="$1"
  local package="thinking-os-core-$version-$TARGET"
  local bundle="$TEMP_DIR/build/$package"
  local release_dir="$RELEASES/v$version"
  mkdir -p "$bundle/bin" "$release_dir"
  cp "$ROOT/install.sh" "$ROOT/VERSION" "$bundle/"
  printf '%s\n' "$version" > "$bundle/VERSION"
  printf '%s\n' "$TARGET" > "$bundle/TARGET"
  printf 'test-commit-%s\n' "$version" > "$bundle/COMMIT"
  cp -R "$ROOT/skills" "$ROOT/template" "$ROOT/deploy" "$bundle/"
  cp "$ROOT/packaging/thinking-os.sh" "$bundle/bin/thinking-os"
  if [ "$version" = 0.0.2 ]; then
    printf '%s\n' 'upstream addition' > "$bundle/template/concepts/upstream-new.md"
  fi
  for tool in sare zaindu ikusi artalde; do
    cat > "$bundle/bin/$tool" <<EOF
#!/bin/bash
set -euo pipefail
if [ "\${1:-}" = --version ] || [ "\${1:-}" = -V ]; then
  printf '%s %s\n' '$tool' '$version'
  exit 0
fi
root="\${THINKING_OS_ROOT:-\$HOME/thinking-os}"
case '$tool' in
  sare)
    case "\${1:-}" in
      build)
        mkdir -p "\$root/graph/views"
        printf 'node\\n' > "\$root/graph/nodes.tsv"
        : > "\$root/graph/wiki-nodes.tsv"
        : > "\$root/graph/wiki-edges.tsv"
        ;;
      fuse) printf 'nodes 1 edges 0 rejected 0\\n' ;;
      serve) mkdir -p "\$root/graph/views" ;;
    esac
    ;;
  zaindu)
    if [ "\${1:-}" = poll ]; then
      mkdir -p "\$root/scripts"
      : > "\$root/scripts/.gh-watch-state.json"
    fi
    printf 'poll complete\\n'
    ;;
  *) printf '%s ok\\n' '$tool' ;;
esac
EOF
    chmod +x "$bundle/bin/$tool"
  done
  chmod +x "$bundle/install.sh" "$bundle/bin/thinking-os"
  tar -czf "$release_dir/$package.tar.gz" -C "$TEMP_DIR/build" "$package"
  if command -v sha256sum >/dev/null 2>&1; then
    (cd "$release_dir" && sha256sum "$package.tar.gz") > "$release_dir/SHA256SUMS"
  else
    (cd "$release_dir" && shasum -a 256 "$package.tar.gz") > "$release_dir/SHA256SUMS"
  fi
}

run_installed() {
  env \
    HOME="$TEST_HOME" \
    PATH="$FAKE_BIN:/usr/bin:/bin" \
    THINKING_OS_BIN_DIR="$TEST_HOME/bin" \
    THINKING_OS_DATA_DIR="$TEST_HOME/data" \
    THINKING_OS_CONFIG_DIR="$TEST_HOME/config" \
    THINKING_OS_RELEASE_BASE_URL="file://$RELEASES" \
    THINKING_OS_ROOT="$TEST_HOME/wiki" \
    THINKING_OS_LATEST_VERSION="$LATEST_VERSION" \
    GIT_AUTHOR_NAME=Test GIT_AUTHOR_EMAIL=test@example.invalid \
    GIT_COMMITTER_NAME=Test GIT_COMMITTER_EMAIL=test@example.invalid \
    "$@"
}

fail() { printf 'test-install.sh: %s\n' "$1" >&2; exit 1; }
assert_file() { [ -f "$1" ] || fail "missing file $1"; }
assert_link() { [ -L "$1" ] || fail "missing symlink $1"; }

LATEST_VERSION=v0.0.1
make_release 0.0.1
cat "$ROOT/install.sh" | run_installed bash -s -- --wiki "$TEST_HOME/wiki" --no-timer > "$TEMP_DIR/install.log" 2>&1 || {
  cat "$TEMP_DIR/install.log" >&2
  fail 'initial binary installation failed'
}
[ ! -e "$TEMP_DIR/cargo-called" ] || fail 'binary installation invoked cargo'
[ ! -e "$TEMP_DIR/rustc-called" ] || fail 'binary installation invoked rustc'
assert_link "$TEST_HOME/data/current"
assert_link "$TEST_HOME/bin/sare"
assert_link "$TEST_HOME/bin/zaindu"
assert_link "$TEST_HOME/bin/ikusi"
assert_link "$TEST_HOME/bin/artalde"
assert_link "$TEST_HOME/bin/thinking-os"
case "$(readlink "$TEST_HOME/.claude/skills/graph")" in
  */current/skills/graph) ;;
  *) fail 'skills are not linked through the active release' ;;
esac
assert_file "$TEST_HOME/wiki/graph/ontology.yaml"
assert_file "$TEST_HOME/config/install.conf"
run_installed "$TEST_HOME/bin/sare" --version | grep -q '0.0.1' || fail 'wrong initial tool version'
case "$(run_installed "$TEST_HOME/bin/thinking-os" version)" in
  *v0.0.1*) ;;
  *) fail 'wrong manager version' ;;
esac

printf 'personal notes\n' > "$TEST_HOME/wiki/notes.md"
printf 'personal instructions\n' > "$TEST_HOME/wiki/CLAUDE.md"
rm -f "$TEST_HOME/.claude/skills/setup"
mkdir -p "$TEST_HOME/.claude/skills/setup"
printf 'local skill\n' > "$TEST_HOME/.claude/skills/setup/local.txt"

make_release 0.0.2
LATEST_VERSION=v0.0.2
run_installed "$TEST_HOME/bin/thinking-os" update > "$TEMP_DIR/update.log" 2>&1 || {
  cat "$TEMP_DIR/update.log" >&2
  fail 'release update failed'
}
run_installed "$TEST_HOME/bin/sare" --version | grep -q '0.0.2' || fail 'update did not activate the new tool'
assert_file "$TEST_HOME/wiki/concepts/upstream-new.md"
[ "$(cat "$TEST_HOME/wiki/notes.md")" = 'personal notes' ] || fail 'update changed a personal page'
[ "$(cat "$TEST_HOME/wiki/CLAUDE.md")" = 'personal instructions' ] || fail 'update overwrote CLAUDE.md'
[ "$(cat "$TEST_HOME/.claude/skills/setup/local.txt")" = 'local skill' ] || fail 'update replaced a local skill'
grep -q 'SAVED_TIMER=0' "$TEST_HOME/config/install.conf" || fail 'update did not preserve --no-timer'
case "$(run_installed "$TEST_HOME/bin/thinking-os" update --check)" in
  *'up to date'*) ;;
  *) fail 'update --check did not recognize the active release' ;;
esac
run_installed "$TEST_HOME/bin/thinking-os" update --version 0.0.2 --no-artalde > "$TEMP_DIR/no-artalde.log" 2>&1 || {
  cat "$TEMP_DIR/no-artalde.log" >&2
  fail 'persisted artalde option update failed'
}
[ ! -e "$TEST_HOME/bin/artalde" ] || fail '--no-artalde did not remove the managed link'
grep -q 'SAVED_ARTALDE=0' "$TEST_HOME/config/install.conf" || fail '--no-artalde was not saved'

make_release 0.0.3
printf 'tampered\n' >> "$RELEASES/v0.0.3/thinking-os-core-0.0.3-$TARGET.tar.gz"
LATEST_VERSION=v0.0.3
if run_installed "$TEST_HOME/bin/thinking-os" update > "$TEMP_DIR/bad-update.log" 2>&1; then
  fail 'checksum failure unexpectedly activated a release'
fi
run_installed "$TEST_HOME/bin/sare" --version | grep -q '0.0.2' || fail 'checksum failure changed the active release'
[ "$(cat "$TEST_HOME/wiki/notes.md")" = 'personal notes' ] || fail 'failed update changed the wiki'

printf 'ok: binary install, persisted update, wiki protection, checksum rollback\n'
