# Artalde UI alignment audit

Date: 2026-11-19. Engine baseline: `ab02f9a238eb2a9bbc780130545e7e67d546520e`. Repairs are uncommitted working-tree changes. The installed `~/.local/bin/artalde` was not replaced. The wiki layouts were updated together with their install templates.

## Findings and repairs

| Finding | Evidence and cause | Repair |
|---|---|---|
| Agent names vertically offset from status dots, indices and ages | Baseline dashboard: nested vertical content starts at the top of a 30-point row | Explicit centered 18/34-point content rectangle for single/two-line agent text |
| Different panel heading baselines | Spaces used default 10-point padding, Agents 14, stacked Session a separate 30-point header | Standardize shipped panel padding to 12; reserve 34 points for titled blocks, matching stack header plus gap |
| Agent toolbar loses space at narrow widths | Tabs, 220-point search and New agent compete on one line | Separate tabs from action/search row; horizontally scroll view tabs; size search from remaining width |
| Flexible center panel collapses when side panels consume the width | `distribute` assigned fixed sizes first, ignored flexible minima/maxima, and allowed overflow | Reserve flexible minima; distribute excess by weight with caps; proportionally shrink an overcommitted split; give Agents a 320-point preferred minimum |
| Fixed row columns can paint beyond their row | Column totals exceed available width; clip only used the child rectangle | Scale overcommitted fixed widths, bound gaps/inset, clip to the parent row too |
| Long repository/resource names cut off mid-glyph | Raw labels inside clipped columns; multiple PR chips have no individual width budget | Truncating repository labels with hover text; divide the resource column into bounded chip widths |
| Card rows vertically center unequal-height cards | `Wrap` used centered horizontal layout | Top-align wrapped cards; wrap mod row children; make grid column count respond to available width |
| Footer chips clipped at the bottom | Status panel was 28/30 points including 10/12 points of padding | 40-point status strip; omit optional shortcut hints below 1100 points |
| Command palette entries overlap status labels | Long selectable labels expanded the modal and collided with right-aligned hints | Use shared full-width rows with separate bounded label/hint columns and hover text |
| Screenshot size was not reproducible | Native window starts at 1440×900; OS accessibility resize did not change captured dimensions | Screenshot-only `ARTALDE_SCREENSHOT_SIZE=WIDTHxHEIGHT`, with finite/bounded parsing; verify actual PNG dimensions. Screenshot-only `ARTALDE_SCREENSHOT_OVERLAY=palette|spawn` opens those dialogs without executing an action |

The first toolbar repair used a full-height `with_layout` inside a vertical parent, which pushed rows out of view. The screenshot pass caught it; the final code uses a bounded horizontal toolbar. Those intermediate images are not final evidence.

## Visual coverage

17 selected native self-screenshots were inspected, including contact sheets for cross-screen comparison and full-size dashboard images. These are live-data captures, not deterministic pixel regression tests. Data and session selection change between runs.

- Dashboard: graphite at 1440×900, paper at 900×650, plus minimum-size check.
- Board (`focus` layout) and Radar: full and reduced-width views, paper/graphite.
- Settings: paper 1000×800 and reduced-width graphite.
- PR inbox: graphite 1440×900, populated live rows.
- Repository: paper 1200×800, merged/open PR listing. The inspected repository has no deployments/releases, so populated versions of those tabs are not certified.
- PR `enekos/thinking-os-core#9`: conversation, files (45-file diff), checks (two passing checks), and empty agent-review composer at 1200×800 in paper/graphite. No review, rerun, merge, or comment was submitted.
- Command palette and spawn dialog: additional inspection captures; no agent was launched.

Evidence directory: `/Users/enekosarasola/thinking-os/reports/artalde-ui-audit-2026-11-19/`. `gallery.html` links the selected decoded PNGs and their actual dimensions. The capture runner and command logs accompany them. Initial OS-resize attempts still produced 1440×900 images and are not counted as narrow-window evidence. Incomplete PNGs from an early runner were discarded; the final runner waits until decoding succeeds before stopping its own process.

## Verification

Run in `thinking-os-core/artalde`:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
ARTALDE_STATE_DIR=/tmp/artalde-ui-audit/check-state \
  ~/.cargo/shared-target/debug/artalde check
```

Results: formatting passed; Clippy passed; 74 tests passed (15 application, 55 core, 4 mods), zero failed; `artalde check` passed all three layouts/themes/mods against live data. Three new regressions cover overcommitted split containment, flexible maximum redistribution, and row-column containment/vertical centers. `git diff --check` passed. Logs apply to the dirty patch, not a new committed revision; no exact-commit verification claim is made.

Reproduce a particular screenshot with isolated state (the directory's `tabs.json` selects the page):

```sh
ARTALDE_STATE_DIR=/tmp/artalde-shot-state \
ARTALDE_CONFIG_DIR="$HOME/thinking-os/artalde" \
ARTALDE_LAYOUT=main ARTALDE_THEME=paper \
ARTALDE_SCREENSHOT_SIZE=900x650 \
ARTALDE_SCREENSHOT=/tmp/artalde-shot.png \
  ~/.cargo/shared-target/debug/artalde
```

On this macOS host a background/occluded test window did not reliably deliver screenshot callbacks. The runner foregrounds only its own process. `ARTALDE_SCREENSHOT_WAIT` delays capture after the initial feed snapshot; it does not prove every remote request has settled. Review loaded content, not just file existence.

## Remaining coverage and follow-up

This pass does **not** certify every UI state as aligned. The following still need the fixture/geometry gate in P0 of `feature-gap-plan.md`:

- 1× scale, alternate display scaling, and the complete 11–18-point font-size matrix. Several custom widgets use hard-coded type sizes instead of the configured scale.
- Populated deployments/releases, failed-check expanded logs, inline review comment forms, large review proposal lists, nested markdown tables/code, all menu/popup states, and long/many tabs.
- Keyboard focus and hit targets, split dragging, live reload with invalid configuration recovery, and bottom-of-scroll settings/session content.
- Stable snapshot fixtures for zero/many sessions, blocked/done transitions, missing terminals, errors, loading and very long names. Live sessions are insufficient for those assertions.
- The full application is macOS-specific in its terminal actions. These captures make no Linux/Windows UI or action claim.

Use the same shared primitives for further fixes. Do not fix each screenshot with unrelated per-panel offsets. Add independent geometry assertions and a small deterministic fixture matrix before expanding the control plane.
