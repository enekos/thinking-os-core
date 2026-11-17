#!/bin/bash
# Event watcher. Polls three GitHub searches (my open PRs, my merged PRs, PRs
# waiting on my review) and, when os.yaml has a `linear:` key, my Linear issues;
# diffs them against scripts/.gh-watch-state.json and writes any transition to
# graph/events/YYYY-MM.tsv + inbox/events/YYYY-MM-DD.md.
# Read-only against both; writes nothing outside this wiki. No notifications.
# Then scripts/autocommit.sh commits ledgers + generated files, hourly.
#
# Scheduled by install.sh: a systemd user timer on Linux, launchd on macOS,
# every 5 minutes.
set -uo pipefail
export PATH="$HOME/.local/bin:$HOME/.cargo/bin:/opt/homebrew/bin:/usr/local/bin:$PATH"

# A timer does not read your shell profile. Credentials the CLIs need from the
# environment (LINEAR_API_KEY, GH_TOKEN) go in this file, mode 600.
ENV_FILE="${THINKING_OS_ENV:-$HOME/.config/thinking-os/env}"
if [ -r "$ENV_FILE" ]; then
  set -a
  # shellcheck disable=SC1090
  . "$ENV_FILE"
  set +a
fi

export WIKI="${THINKING_OS_ROOT:-$(dirname "$(dirname "$(realpath "$0")")")}"
LOG="$WIKI/scripts/zaindu-poll.log"

OUT=$(zaindu poll --root "$WIKI" 2>&1)
STATUS=$?
printf '%s %s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$(printf '%s' "$OUT" | tr '\n' ' ')" >> "$LOG"
tail -n 500 "$LOG" > "$LOG.tmp" && mv "$LOG.tmp" "$LOG"
"$WIKI/scripts/autocommit.sh" >> "$LOG" 2>&1
exit $STATUS
