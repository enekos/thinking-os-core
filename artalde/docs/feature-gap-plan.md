# Artalde feature gap and implementation plan

Date: 2026-11-19. Target: thinking-os-core `ab02f9a` plus the local UI repair described in `ui-audit.md`. This is a proposed roadmap, not a claim that the features below have shipped.

## Recommendation

Make Artalde a reliable, provider-aware thinking-os workbench first: consistent UI, trustworthy session state, durable attention, worktree changes, and links from work to its verification evidence. Keep the native egui window, wiki-owned JSON/Rhai customization, terminal independence, and explicit human submission of reviews. Add managed provider sessions as an opt-in capability later. Remote execution and embedded terminals are separate product decisions.

## Comparison baseline

Inspected source checkouts, not just marketing pages:

- [Herdr](https://github.com/herdrdev/herdr-app/tree/317e673f1f1568b876cbaa0b41dabeb9153ecbea), revision `317e673f1f1568b876cbaa0b41dabeb9153ecbea`. **Current Herdr uses Rust/GPUI, not egui** (`Cargo.toml`). It delegates terminal execution to herdr-core; this audit inspected herdr-app, not the core runtime. Evidence: `src/control.rs`, `src/layout_sync.rs`, `src/persist.rs`, `src/mods/render_tests.rs`, `docs/commands.md`, README.
- [T3 Code](https://github.com/pingdotgg/t3code/tree/475ae52ac279152dd2c13a86a59227d3e1441e3d), revision `475ae52ac279152dd2c13a86a59227d3e1441e3d`. Evidence: `docs/internals/overview.md`, `docs/user/{composer,portable-handoffs,source-control,usage,triage}.md`, `apps/server/src/{orchestration-v2,checkpointing,scheduledTasks,sourceControl}`, `packages/contracts/src/providerPolicy.ts`.
- Artalde source: `crates/artalde-core/src/{model,hook,probe,claude,store,layout,command,review}.rs`; `crates/artalde/src/{feed,actions,views,reviewer,repo_view,settings_view,kit,layout_view,mod_view}.rs`; actual wiki layouts and mods. Reference applications were source-audited, not launched or behavior-certified.

Status vocabulary: **present** = implemented, not necessarily exercised end-to-end; **partial** = useful implementation with a specific missing capability; **missing** = no corresponding implementation found in the inspected command/model/view paths; **different scope** = incompatible with the current ownership model unless deliberately added.

## Feature matrix

| Capability | Reference behavior | Artalde finding | Priority |
|---|---|---|---|
| Attention and session identity | Herdr working/blocked/done/idle, workers and focus | **Present/partial:** attention groups, tags, labels, terminal focus, Claude titles; generic process discovery does not establish provider state | P1 |
| Multiple providers | T3 normalized provider adapters and capabilities | **Partial:** discovers claude/codex/opencode and accepts generic reports; richer identity, transcript and tasks are Claude-specific | P1 |
| Resume, stop, fork, model/effort change | T3 controlled conversations and portable handoffs | **Missing:** terminal reply/keys are not lifecycle APIs; no provider capability contract | P4 |
| Permissions and questions | T3 structured approval modes and requests | **Partial:** blocked messages plus terminal keys; no typed request ID, expiry, choices or stale-response guard | P4 |
| Conversation UI | T3 rich composer, attachments and activity | **Partial:** bounded recent Claude text transcript and reply; missing normalized tool events, attachment handling, draft persistence and history search | P2/P4 |
| Parent/worker hierarchy | Herdr workers/peek; T3 subagent orchestration | **Missing:** flat session list, no parent-session identity or completion relationship | P2 |
| Durable attention | Herdr viewed/done semantics; T3 durable settlement | **Partial:** in-memory acknowledgement and notifications; no persisted attention cursor/snooze/history | P2 |
| Worktrees | Herdr spaces; T3 thread worktree lifecycle | **Partial:** discover git worktrees, dirty/ahead/behind and spawn via `zz`; no inspected cleanup, branch checkout or tracked operation lifecycle | P3 |
| Local changes and checkpoints | T3 local diff, commit/push/PR and coordinated rollback | **Partial/missing:** remote PR diff is present; local working-tree diff/staging/checkpoints absent | P3/P5 |
| Pull-request review | T3 forge client | **Present:** inbox/filtering, conversation/files/checks, comments, review threads, merge actions and agent proposals. Do not rebuild this | Harden |
| Repository delivery | Repo PR/deployment/release visibility | **Present:** repository tabs. Deployment visibility is not deployment execution | Keep |
| Work-to-knowledge links | thinking-os tasks, attempts, tests and graph | **Partial:** transcript-derived PR/ticket resources and zaindu inbox; no explicit task/attempt/test provenance workflow | P2/P3 |
| Layout persistence | Herdr click state separate from layout definitions | **Partial:** top-level tabs persist; split sizes/stack selection live in `LayoutState` memory | P2 |
| Customization | Herdr hot-reloaded layouts/mods/keys | **Present:** JSON layouts/themes/keys, Rhai budgets and last-good edits. No reason to replace with JS | Keep |
| Scriptable UI inspection/control | Herdr socket `do`, `ls`, `status`, screenshot support | **Partial:** snapshot/check/commands/headless PR reads, no external dispatcher for the running UI or widget geometry inventory | P0/P2 |
| Terminal persistence and peek | Herdr core owns PTYs across window close | **Different scope:** Artalde does not own terminals; reuse the user's terminal and optionally add a read-only session peek | Optional |
| Remote/multiple machines | Herdr SSH cores; T3 authenticated clients/environment ownership | **Missing/different scope:** local machine, macOS actions, no authenticated transport | P6 |
| Usage/cost/context limits | T3 usage and quota views | **Missing:** model has no normalized usage/cost/freshness fields | P5 |
| Scheduling and hooks into workflows | T3 scheduled task service/webhooks | **Partial:** zaindu already watches external events; no Artalde durable scheduled execution | P5 |
| Diagnostics/onboarding | Herdr core readiness; T3 provider readiness/triage | **Partial:** hook settings, logs/errors and `check`; no capability health report or redacted diagnostic bundle | P1 |
| Rich extras | T3 mobile, browser, voice, sharing, secrets, alternative forges | **Missing; defer:** not prerequisites for a local thinking-os control plane | Optional |

## Architecture contracts

1. **Preserve the three-crate boundary.** Validated identifiers, normalized events and pure transitions in `artalde-core`; bounded Rhai descriptions in `artalde-mods`; egui rendering and adapter wiring in `artalde`. Introduce another crate only for a real runtime/dependency boundary.
2. **Keep sources authoritative.** Wiki owns task/attempt/test pages and UI definitions. Provider owns its session. Git owns commits/worktrees. `gh` owns GitHub mutations. `zaindu` owns proposals. Cache their views without inventing a competing source of truth. Use `sare` for derived graph views; never hand-edit generated TSV/index files.
3. **Typed identities and capabilities.** Separate `SessionId`, `ProviderSessionId`, `RequestId`, `TaskId`, `WorktreePath`, `CommitId`. Parse external strings at adapters. Distinguish observed sessions from managed sessions. A capability enum drives available actions; unsupported operations explain why rather than silently sending terminal keystrokes.
4. **Explicit state and freshness.** Separate provider state, inferred state, last observation, last transition, and attention acknowledgement. A stale process is not evidence of success. Use state-owned payloads for pending questions and operations; reject stale request responses.
5. **One owner per worker.** Bounded queues, bounded input/output, subprocess deadlines, explicit cancellation and joined shutdown. Existing unbounded control/result channels and subprocess waits need an audit before adding more long-running work. UI code never waits on git, providers or network.
6. **Mutations are reviewable.** Keep agent findings as proposals. A command receipt means accepted, not completed. Give mutations operation IDs and explicit results; reconcile ambiguous outcomes instead of automatically retrying a merge/comment/commit. Persist intent/results only where crash recovery requires it; do not transplant all of T3's event-sourcing machinery.
7. **Files remain portable.** Version app-owned persisted state, atomically replace it, preserve unknown/old data during migration, and keep transient state out of wiki definitions. No transcripts or diffs in the knowledge graph.
8. **Verification follows the house guides:** `rust-style-guide`, `rust-type-driven-design`, `rust-resource-and-concurrency-patterns`, `architecture-fitness-and-quality-gates`, `verification-patterns`. Record exact commands and revision/dirty patch evidence; recommendations are not passed tests.

## Delivery sequence

### P0 — Layout correctness and reproducible visual verification

Scope: `kit.rs`, `layout_view.rs`, `panels.rs`, `mod_view.rs`, `settings_view.rs`, `views.rs`, and both wiki/template layouts.

- Repair measured clipping and baseline errors before adding new panels. Shared spacing, consistent block/stack headers, row content rectangles, width budgets, responsive toolbars and a readable narrow-window strategy.
- Reproducible screenshots specify config, state, theme, viewport, route, fixture and clock. Capture actual dimensions; a requested resize is not evidence that it happened. Wait for decoded PNG completion and visible data/error state.
- Add a fixture-backed rendering harness for empty/loading/error/blocked/done/long-text states, stable widget IDs, and exported rectangles. Assert containment, row centers and non-overlap independently of pixel snapshots. Keep live screenshots as supplementary evidence.
- Gate: all three layouts and major tabs in paper/graphite at 760×480, 1024×768 and 1440×900; 1×/2× scale and 11/13.5/18 pt; split resize, tabs, focus, mod reload, expanded rows and scroll endpoints. Aim for <=1 logical pixel mismatch for intended shared baselines/edges.
- Current repair and visual evidence are tracked separately in `ui-audit.md`; do not treat this entire gate as complete merely because representative screenshots pass.

### P1 — Provider observation and diagnostics

Depends on P0. Scope: `model.rs`, `probe.rs`, `claude.rs`, `hook.rs`, `feed.rs`, CLI and settings.

- Extract Claude observation behind a small provider observation port. Add Codex observation/report adapter first, then OpenCode; keep process discovery as explicitly inferred fallback.
- Normalize identity, workspace, title, state, tool activity, parent ID, timestamps and provenance. Resolve duplicate hook/probe records with a stable provider ID. Expose supported capabilities and missing executable/auth/hook/terminal prerequisites.
- Add `artalde doctor --json` and a UI health view. Never install hooks or change global provider settings as a side effect of diagnosis.
- Acceptance: fixtures replay transitions for each adapter; duplicate/out-of-order reports and PID reuse do not fabricate a new completion; corrupt/oversized/transcript-rotation inputs stay bounded. Real provider observation conformance runs remain a separate local check.

### P2 — Attention, session history and thinking-os context

Depends on P1. Scope: core model/store/catalog, session views and mods context.

- Persist acknowledgement by `(session ID, completion generation)` so an old acknowledgement cannot hide a new completion. Add snooze, pin, archive and explicit unread state, plus deduplicated notifications.
- Normalize and paginate conversation/tool events. Add search, a multiline draft, copy/link actions, read-only peek and parent/worker grouping; retain external-terminal focus.
- Expose task/attempt/test references and evidence freshness from existing wiki/graph records. Explicitly associate a session with a task when inferred links are ambiguous.
- Persist split sizes and stack selection separately from layout JSON, invalidating overrides when layout structure changes. Keep top-level tab persistence.
- Optional local control socket reuses the command registry, with bounded messages, same-user access, typed responses and explicit mutation classification. Start with inspect/select/open/capture.
- Acceptance: restart preserves unread/drafts/layout state; dismissing completion A does not dismiss B; deleted sessions/changed layouts recover; task links identify the correct repo and revision. Full transcripts remain outside the graph.

### P3 — Worktree changes and delivery evidence

Depends on P1/P2. Scope: new core git operation/model modules; adapter worker; local changes view; existing `zz`, PR diff renderer and `gh` paths.

- Add read-only working-tree diff, staged/unstaged/untracked categories, hunk navigation and comparison to branch base. Reuse diff rendering without conflating local changes with PR head diffs.
- Add inspectable worktree create/checkout/remove operations around existing conventions. Block removal when a live session or uncommitted files exist; offer an explicit recovery path.
- Add stage/unstage, commit, push and draft-PR proposals with captured repo/worktree/head revision. Revalidate before execution; preserve user edits and show conflict outcomes.
- Connect completed work to task commit SHAs, attempts and test evidence; reuse the existing wrap/quality-gate workflow rather than another autonomous publisher.
- Acceptance: disposable real git repositories cover detached HEAD, dirty/untracked files, rename/binary/large diff, concurrent branch movement, command failure and cancellation. PR publishing and review submission remain explicit actions.

### P4 — Optional managed sessions and structured approvals

Depends on P1/P2. This adds an execution mode; observed terminal sessions continue to work.

- Define a provider control port: start/send/interrupt/resume with advertised capabilities; extend only when a provider supports fork, model selection, attachments or structured approvals.
- Bind each request to provider session, run and request IDs; show tool, scope, choices and expiry. Reject responses to superseded requests. A terminal-only session offers terminal focus, never a fabricated structured approval.
- Add queued follow-ups, attachment bounds, model/effort selectors and resumable drafts. Keep process/session ownership explicit and cancellation results visible.
- Implement provider switching as a reviewed, bounded context handoff. Do not copy hidden reasoning or tool state; retain full current user input and references to omitted history. Unsupported resume/fork is a distinct capability, not a silent fallback.
- Acceptance: fake and real adapters share contract cases; cancellation during startup/tool wait/approval, restart, duplicate dispatch and ambiguous failures are covered. No duplicate side effects after recovery.

### P5 — Checkpoints, usage and automation

Depends on P3/P4.

- Checkpoint local changes through hidden Git refs with retention and size limits; preview the restore. Coordinate workspace and conversation rollback; reject before filesystem changes if the provider cannot restore the corresponding conversation.
- Add usage/context/quota snapshots with units, source, observation time and unknown values. Do not infer billing from token counts without an explicit price source.
- Reuse zaindu events and existing schedulers for triggers. Artalde initially proposes actions; any later unattended executor needs durable receipts, deduplication, concurrency limits and recovery tests.
- Evaluate agent-review proposals on a small labeled corpus: finding correctness, location correctness, missed severe defects, duplicate findings, and user acceptance/dismissal reasons. Keep generation quality separate from deterministic orchestration tests. Add observability only at a measured need; no new hosted dependency is required for the first release.
- Acceptance: crash during capture/restore cannot discard user changes, usage staleness is visible, repeated webhook/timer delivery starts at most one logical operation, and evaluation cases retain failure evidence.

### P6 — Remote environments only after local ownership is stable

Depends on P4 and a separate product decision.

- Add explicit machine/environment identity to sessions, worktrees and commands before introducing transport. Keep credentials, git and provider execution on the owning machine.
- Prefer an authenticated local/SSH transport first. Reconnect must converge from a snapshot/cursor and must not replay non-idempotent actions. Do not use a remote machine's path on the local filesystem.
- Mobile/web clients, shared access, embedded terminal emulation and provider secrets are later projects with their own boundaries and acceptance tests.

## First implementation tickets

| Ticket | Concrete output | Exit evidence |
|---|---|---|
| UI geometry | Shared layout fixes and saved visual matrix | Containment/overflow regressions + reviewed screenshots |
| Render fixtures | Reproducible states/routes without real actions | One command captures matrix and rejects incomplete PNGs |
| Provider observation | Claude port + Codex adapter and provenance | Transition replay/conformance suite |
| Durable attention | Persisted completion acknowledgement/snooze | Restart and new-generation tests |
| Knowledge context | Explicit task/test links on session | Correct repo/SHA and stale-evidence fixtures |
| Local changes | Working-tree diff and safe worktree actions | Disposable git repository integration suite |
| Managed sessions | Capability-gated lifecycle and approvals | Stale approval/cancel/recovery contract suite |

Deliver these as individually reviewable changes in the order above. Do not begin all integrations at once. Estimated effort needs a short spike for the chosen Codex/Claude control protocols and screenshot harness; feature count alone is not a credible schedule.
