#!/bin/bash
set -uo pipefail
export PATH="/opt/homebrew/bin:/usr/local/bin:$HOME/.cargo/bin:$PATH"

WIKI="$(dirname "$(dirname "$(realpath "$0")")")"
CRATE="$WIKI/tools"
FIX="$CRATE/tests/fixtures"
TARGET="$(cargo metadata --manifest-path "$WIKI/tools/Cargo.toml" --format-version 1 --no-deps 2>/dev/null | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')"
SARE="$TARGET/debug/sare"
BIN="$(cargo metadata --manifest-path "$CRATE/Cargo.toml" --format-version 1 --no-deps 2>/dev/null | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')/debug/zaindu"
PASS=0; FAIL=0

ok()   { PASS=$((PASS+1)); printf '  ok   %s\n' "$1"; }
bad()  { FAIL=$((FAIL+1)); printf '  FAIL %s\n' "$1"; [ $# -gt 1 ] && printf '       %s\n' "$2"; }
is()   { if [ "$2" = "$3" ]; then ok "$1"; else bad "$1" "want [$3] got [$2]"; fi; }
has()  { if printf '%s' "$2" | grep -qF -- "$3"; then ok "$1"; else bad "$1" "missing [$3]"; fi; }
hasnt(){ if printf '%s' "$2" | grep -qF -- "$3"; then bad "$1" "unexpected [$3]"; else ok "$1"; fi; }

seed_root() {
  local root="$1"
  rm -rf "$root"; mkdir -p "$root/graph/raw"
  cp "$WIKI/template/graph/ontology.yaml" "$WIKI/template/graph/services.tsv" "$root/graph/"
  # four wiki projects claim the fixture monorepo: proposals name two, count the rest
  printf 'id\ttype\ttitle\n' > "$root/graph/nodes.tsv"
  printf 'src\trel\tdst\n' > "$root/graph/edges.tsv"
  for p in alpha-app beta-app gamma-app delta-app; do
    printf '%s\tproject\t%s\n' "$p" "$p" >> "$root/graph/nodes.tsv"
    printf '%s\tLIVES_IN\tgh:join-com/data\n' "$p" >> "$root/graph/edges.tsv"
  done
}

poll() {
  local root="$1"; shift
  "$BIN" poll --root "$root" --state "$root/state.json" --json "$@"
}

# fixtures are frozen in time: widen the merged/closed window so they never age out
fixture_poll() {
  local root="$1"; shift
  ZAINDU_LOGIN=enekos poll "$root" --since-days 36500 --fixture "$@"
}

kinds() { python3 -c 'import json,sys; print(" ".join(sorted(e["kind"] for e in json.load(sys.stdin)["events"])))'; }
field() { python3 -c "import json,sys; print(json.load(sys.stdin)[\"$1\"])"; }

echo "building"
cargo build --manifest-path "$CRATE/Cargo.toml" 2>&1 | tail -2
[ -x "$BIN" ] || { echo "no binary at $BIN"; exit 1; }

echo
echo "unit tests"
cargo test --manifest-path "$CRATE/Cargo.toml" 2>&1 | tail -3

echo
echo "e2e / offline — fixtures through the whole chain"
ROOT="/tmp/zaindu-e2e-offline"
seed_root "$ROOT"

OUT=$(fixture_poll "$ROOT" "$FIX/1-before.json")
is "first run seeds, emits nothing" "$(printf '%s' "$OUT" | field seeded)" "True"
is "  and watches the whole fixture" "$(printf '%s' "$OUT" | field watched)" "5"
is "  with no events" "$(printf '%s' "$OUT" | kinds)" ""

OUT=$(fixture_poll "$ROOT" "$FIX/2-after.json")
is "second poll fires one event per real transition" "$(printf '%s' "$OUT" | kinds)" \
   "pr.approved pr.changes_requested pr.checks_red pr.closed pr.merged pr.review_requested"

OUT=$(fixture_poll "$ROOT" "$FIX/2-after.json")
is "the same state again is silent" "$(printf '%s' "$OUT" | kinds)" ""

OUT=$(fixture_poll "$ROOT" "$FIX/3-recovered.json")
is "a recovery fires once" "$(printf '%s' "$OUT" | kinds)" "pr.checks_green"

LEDGER_FILE="$ROOT/graph/events/$(date -u +%Y-%m).tsv"
LEDGER=$(cat "$LEDGER_FILE")
is "the ledger has a header and one row per event" "$(printf '%s\n' "$LEDGER" | wc -l | tr -d ' ')" "8"
has "the ledger is grep-able by node id" "$LEDGER" "gh:join-com/data#7276"
is "every ledger row has 7 columns" \
   "$(awk -F'\t' 'NF!=7{n++} END{print n+0}' "$LEDGER_FILE")" "0"

INBOX=$(cat "$ROOT/inbox/events/$(date +%F).md")
is "each actionable event becomes one unchecked box" \
   "$(printf '%s\n' "$INBOX" | grep -c '^- \[ \] ')" "6"
is "a recovery is a timeline line, not a task" \
   "$(printf '%s\n' "$INBOX" | grep -c '^- [0-9]')" "1"
has "a merge proposes the wiki bookkeeping" "$INBOX" "tick Next up, add a Recent log bullet"
has "the repo is resolved to its project page" "$INBOX" "[[alpha-app]]"
has "  and a monorepo's other claimants are counted, not listed" "$INBOX" "+2 more"
has "a review request names who is blocked" "$INBOX" "you are blocking @colleague"
has "a red check names the job" "$INBOX" "checks red: ci/circleci: test"
hasnt "a stale rerun is never reported red" "$INBOX" "lint-pr-title"

echo
echo "e2e / offline — the events reach the graph"
"$SARE" fuse --root "$ROOT" > "$ROOT/fuse.log" 2>&1
is "fusion accepts the watch source" "$?" "0"
MERGED=$(awk -F'\t' '$1=="gh:join-com/data#7276"' "$ROOT/graph/nodes.tsv")
has "a merged PR survives as a node" "$MERGED" "gh:join-com/data#7276"
has "  with its real title, not a placeholder" "$MERGED" "stop an unmatched anchor"
has "  and status merged" "$MERGED" "merged"
EDGES=$(cat "$ROOT/graph/edges.tsv")
has "the PR keeps its repo edge" "$EDGES" "OPENED_IN"
has "the reviewer edge survives fusion" "$EDGES" "REVIEWS"
is "no watch edge is rejected" \
   "$(awk '/zaindu/{n++} END{print n+0}' "$ROOT/graph/raw/rejected.tsv" 2>/dev/null)" "0"

echo
echo "e2e / offline — the read commands"
has "events replays the ledger" "$("$BIN" events --root "$ROOT")" "pr.merged"
is "events filters by kind" "$("$BIN" events --root "$ROOT" --kind merged | wc -l | tr -d ' ')" "1"
has "proposals prints today's open boxes" "$("$BIN" proposals --root "$ROOT")" "- [ ]"
has "status reads the snapshot back" "$("$BIN" status --root "$ROOT" --state "$ROOT/state.json")" "join-com/data#7280"

if [ "${ZAINDU_E2E_OFFLINE:-0}" = "1" ]; then
  echo; echo "live leg skipped (ZAINDU_E2E_OFFLINE=1)"
else
  echo
  echo "e2e / live — real GitHub, real state"
  LIVE="/tmp/zaindu-e2e-live"
  seed_root "$LIVE"

  OUT=$(poll "$LIVE")
  RC=$?
  is "a live poll succeeds" "$RC" "0"
  WATCHED=$(printf '%s' "$OUT" | field watched)
  if [ "${WATCHED:-0}" -gt 0 ]; then ok "  and watches $WATCHED real PRs"; else bad "  and watches real PRs" "watched=$WATCHED"; fi
  is "  seeding, so it stays silent" "$(printf '%s' "$OUT" | field seeded)" "True"

  OUT=$(poll "$LIVE")
  is "an immediate second live poll is silent" "$(printf '%s' "$OUT" | kinds)" ""

  python3 - "$LIVE/state.json" <<'PY'
import json, sys
p = sys.argv[1]
s = json.load(open(p))
for pr in s["prs"].values():
    pr["lifecycle"] = "open"
    pr["review"] = "none"
    pr["review_by"] = []
    pr["checks"] = "green"
    pr["failing"] = []
    pr["review_requested_of_me"] = False
json.dump(s, open(p, "w"))
PY
  OUT=$(poll "$LIVE")
  COUNT=$(printf '%s' "$OUT" | python3 -c 'import json,sys; print(len(json.load(sys.stdin)["events"]))')
  if [ "${COUNT:-0}" -gt 0 ]; then ok "rewinding real state replays $COUNT real transitions"; else bad "rewinding real state replays transitions" "0 events"; fi
  KINDS=$(printf '%s' "$OUT" | kinds)
  has "  including a real merge" "$KINDS" "pr.merged"
  LIVE_INBOX=$(cat "$LIVE/inbox/events/$(date +%F).md" 2>/dev/null)
  has "  written as proposals" "$LIVE_INBOX" "- [ ] "
  "$SARE" fuse --root "$LIVE" > "$LIVE/fuse.log" 2>&1
  is "  and fused into a real graph" "$?" "0"
  REAL_MERGED=$(awk -F'\t' '$2=="pr" && $5=="merged" && $3!=""' "$LIVE/graph/nodes.tsv" | wc -l | tr -d ' ')
  if [ "${REAL_MERGED:-0}" -gt 0 ]; then ok "  $REAL_MERGED merged PRs now carry a title in nodes.tsv"; else bad "merged PRs carry a title" "none found"; fi
fi

echo
echo "$PASS passed, $FAIL failed"
[ "$FAIL" -eq 0 ]
