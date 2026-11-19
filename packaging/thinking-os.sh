#!/bin/bash
# Installed release manager. The same file lives in packaging/ in a source
# checkout and in bin/ in a release bundle.
set -euo pipefail

SOURCE="${BASH_SOURCE[0]}"
while [ -L "$SOURCE" ]; do
  DIR="$(cd -P "$(dirname "$SOURCE")" && pwd)"
  LINK="$(readlink "$SOURCE")"
  case "$LINK" in /*) SOURCE="$LINK" ;; *) SOURCE="$DIR/$LINK" ;; esac
done
SCRIPT_DIR="$(cd -P "$(dirname "$SOURCE")" && pwd)"
CORE="$(cd "$SCRIPT_DIR/.." && pwd -P)"
REPOSITORY="${THINKING_OS_RELEASE_REPOSITORY:-enekos/thinking-os-core}"

installed_version() {
  if [ ! -f "$CORE/VERSION" ]; then
    printf 'thinking-os: cannot find VERSION next to the installation\n' >&2
    exit 1
  fi
  printf 'v%s' "$(cat "$CORE/VERSION")"
}

latest_version() {
  if [ -n "${THINKING_OS_LATEST_VERSION:-}" ]; then
    printf '%s\n' "${THINKING_OS_LATEST_VERSION#v}"
    return
  fi
  gh api "repos/$REPOSITORY/releases/latest" --jq .tag_name | sed 's/^v//'
}

usage() {
  cat <<'EOF'
thinking-os — manage the installed thinking-os-core release

  thinking-os update [--version X.Y.Z]  install the latest (or selected) release
  thinking-os update --check            report versions without changing anything
  thinking-os version [--check]         print the installed (and latest) version

Update uses the wiki, skills, timer, and artalde choices saved by install.sh.
EOF
}

case "${1:-}" in
  update)
    shift
    if [ "${1:-}" = "--check" ]; then
      shift
      [ $# -eq 0 ] || { usage >&2; exit 2; }
      INSTALLED="$(installed_version)"
      LATEST="v$(latest_version)"
      printf 'installed %s\nlatest    %s\n' "$INSTALLED" "$LATEST"
      [ "$INSTALLED" = "$LATEST" ] && printf 'up to date\n' || printf 'update available: thinking-os update\n'
      exit 0
    fi
    exec "$CORE/install.sh" --update "$@"
    ;;
  version|--version|-V)
    INSTALLED="$(installed_version)"
    if [ -f "$CORE/TARGET" ]; then
      printf 'thinking-os-core %s (%s)\n' "$INSTALLED" "$(cat "$CORE/TARGET")"
    else
      printf 'thinking-os-core %s (source)\n' "$INSTALLED"
    fi
    [ -f "$CORE/COMMIT" ] && printf 'commit    %s\n' "$(cat "$CORE/COMMIT")"
    if [ "${2:-}" = "--check" ]; then
      printf 'latest    v%s\n' "$(latest_version)"
    elif [ $# -gt 1 ]; then
      usage >&2
      exit 2
    fi
    ;;
  help|--help|-h|'') usage ;;
  *) usage >&2; exit 2 ;;
esac
