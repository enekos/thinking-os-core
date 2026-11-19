#!/bin/bash
# Build one complete thinking-os-core release bundle from already-built binaries.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
VERSION="$(cat "$ROOT/VERSION")"
OUT="$ROOT/dist"
TARGET=""

while [ $# -gt 0 ]; do
  case "$1" in
    --version) VERSION="$2"; shift 2 ;;
    --target) TARGET="$2"; shift 2 ;;
    --out) OUT="$2"; shift 2 ;;
    -h|--help)
      printf 'usage: %s [--version X.Y.Z] [--target TARGET] [--out DIR]\n' "$0"
      exit 0
      ;;
    *) printf 'build-release.sh: unknown flag %s\n' "$1" >&2; exit 2 ;;
  esac
done

case "$VERSION" in
  ''|*[!0-9A-Za-z.-]*)
    printf 'build-release.sh: invalid version %s\n' "$VERSION" >&2
    exit 2
    ;;
esac
VERSION="${VERSION#v}"
if [ -n "$TARGET" ]; then
  case "$TARGET" in
    *[!0-9A-Za-z_.-]*)
      printf 'build-release.sh: invalid target %s\n' "$TARGET" >&2
      exit 2
      ;;
  esac
fi
if [ -z "$TARGET" ]; then
  case "$(uname -s)-$(uname -m)" in
    Linux-x86_64) TARGET=x86_64-unknown-linux-gnu ;;
    Darwin-arm64|Darwin-aarch64) TARGET=aarch64-apple-darwin ;;
    *) printf 'build-release.sh: unsupported host %s\n' "$(uname -sm)" >&2; exit 1 ;;
  esac
fi

PACKAGE="thinking-os-core-$VERSION-$TARGET"
mkdir -p "$OUT"
STAGING="$OUT/$PACKAGE"
ARCHIVE="$OUT/$PACKAGE.tar.gz"
case "$STAGING" in
  "$OUT"/thinking-os-core-*) ;;
  *) printf 'build-release.sh: unsafe staging path %s\n' "$STAGING" >&2; exit 1 ;;
esac
rm -rf "$STAGING"
mkdir -p "$STAGING/bin"

for tool in sare zaindu ikusi; do
  binary="$ROOT/tools/target/release/$tool"
  [ -x "$binary" ] || { printf 'build-release.sh: missing %s; cargo build --release --locked --manifest-path tools/Cargo.toml\n' "$binary" >&2; exit 1; }
  cp "$binary" "$STAGING/bin/$tool"
done
binary="$ROOT/artalde/target/release/artalde"
[ -x "$binary" ] || { printf 'build-release.sh: missing %s; cargo build --release --locked --manifest-path artalde/Cargo.toml --package artalde\n' "$binary" >&2; exit 1; }
cp "$binary" "$STAGING/bin/artalde"

cp "$ROOT/packaging/thinking-os.sh" "$STAGING/bin/thinking-os"
cp "$ROOT/install.sh" "$STAGING/"
printf '%s\n' "$VERSION" > "$STAGING/VERSION"
cp -R "$ROOT/skills" "$ROOT/template" "$ROOT/deploy" "$STAGING/"
printf '%s\n' "$TARGET" > "$STAGING/TARGET"
git -C "$ROOT" rev-parse HEAD > "$STAGING/COMMIT"
chmod +x "$STAGING/install.sh" "$STAGING/bin/"*

tar -czf "$ARCHIVE" -C "$OUT" "$PACKAGE"
if command -v sha256sum >/dev/null 2>&1; then
  (cd "$OUT" && sha256sum "$PACKAGE.tar.gz") > "$ARCHIVE.sha256"
else
  (cd "$OUT" && shasum -a 256 "$PACKAGE.tar.gz") > "$ARCHIVE.sha256"
fi
printf '%s\n' "$ARCHIVE"
