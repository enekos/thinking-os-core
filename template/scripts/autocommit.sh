#!/bin/bash
# Commits the machine-written part of the wiki, at most once an hour, so the
# working tree never piles up weeks of generated churn into one huge diff.
#
# Only these paths, and only on the default branch: the event ledgers, the
# generated views and index, and the wiki's own graph projection. Pages,
# sources and anything you have staged are never touched — `git commit --only`
# commits the listed paths and leaves the rest of the index as it was.
# Never pushes.
set -uo pipefail

WIKI="${WIKI:-$(dirname "$(dirname "$(realpath "$0")")")}"
STAMP="$WIKI/scripts/.autocommit-stamp"
EVERY=3600
PATHS=(graph/events inbox/events graph/views index.md graph/wiki-nodes.tsv graph/wiki-edges.tsv)

git() { command git -C "$WIKI" "$@"; }

branch=$(git branch --show-current)
case "$branch" in main|master) ;; *) exit 0 ;; esac
for marker in rebase-merge rebase-apply MERGE_HEAD; do
  [ -e "$WIKI/.git/$marker" ] && exit 0
done

mtime() { stat -c %Y "$1" 2>/dev/null || stat -f %m "$1"; }
if [ -f "$STAMP" ]; then
  age=$(( $(date +%s) - $(mtime "$STAMP") ))
  [ "$age" -ge "$EVERY" ] || exit 0
fi

existing=()
for p in "${PATHS[@]}"; do [ -e "$WIKI/$p" ] && existing+=("$p"); done
[ ${#existing[@]} -gt 0 ] || exit 0
git add -- "${existing[@]}" 2>/dev/null
changed=$(git diff --cached --name-only -- "${existing[@]}" | wc -l | tr -d ' ')
touch "$STAMP"
[ "$changed" -gt 0 ] || exit 0

git commit -q --only -m "auto: ledgers + generated graph ($changed files)" -- "${existing[@]}"
