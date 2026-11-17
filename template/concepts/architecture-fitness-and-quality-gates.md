---
title: Architecture fitness and quality gates
summary: "turn architecture rules into dependency checks, API budgets, justified suppressions, risk-based local and CI gates, actionable code review, and exact-commit verification evidence"
type: concept
updated: 2026-11-17
edges:
  - "APPLIES_TO sutegi"
  - "APPLIES_TO osagai"
---

# Architecture fitness and quality gates

An architecture rule is durable only when it has an owner and a check. A review
comment repeated every month is a candidate fitness function. This page defines
the policy; `/quality-gate` applies it to a diff and
[[verification-patterns]] selects the test oracle.

## Write rules as executable contracts

Each rule needs:

| Field | Question |
|---|---|
| Invariant | What must always remain true? |
| Scope | Which crates/packages/layers does it cover? |
| Detector | Which compiler, lint, test, or small script detects violation? |
| Trigger | Fast, merge, scheduled, or only when relevant files change? |
| Exception | Who approved it, why, and when does it expire? |
| Failure action | What exact code change restores compliance? |

Do not invent a custom lint for a one-off preference. Automate rules that are
repeated, objectively detectable, and cheap to maintain. Review judgment remains
for naming, unnecessary abstraction, and ambiguous product semantics.

## Dependency direction and module boundaries

### TypeScript/Svelte

The default graph is `api → application → domain` and
`infrastructure → application`; domain imports neither outer layer.

Enforce with the repository's established `eslint-plugin-boundaries`,
`dependency-cruiser`, Nx module-boundary rule, or equivalent. One maintained
system is enough.

- Domain cannot import framework, transport, ORM, environment, or adapter code.
- Application ports cannot import their infrastructure implementations.
- Cross-feature imports go through the feature's public entry point.
- Shared/kernel modules cannot import features.
- Tests may compose adapters but cannot grant production code a backdoor import.
- Cycles are errors unless an explicitly reviewed, temporary exception exists.

Package `exports` defines the package boundary; deep imports into another
package's `src/` are violations. Avoid broad barrels that accidentally connect
features or hide cycles.

### Rust

- Default to private or `pub(crate)` visibility. `lib.rs` deliberately exposes
  the crate API; binary argument parsing and wiring stay thin.
- Keep serialization/FFI/database representations inside their adapter modules
  unless they are intentionally the public domain model.
- Workspace dependencies follow the crate's purpose. A domain crate gaining a
  web framework or async runtime requires an architectural explanation.
- Use `cargo metadata`/dependency tooling to inspect unexpected edges and
  `cargo public-api diff` for published surfaces.
- Prefer actual visibility over a custom import scanner. Add a scanner only for
  workspace-level rules Cargo cannot express.

## Structural and resource budgets

Budgets encode failure modes, not aesthetic preferences.

| Budget | Example detector | Review question |
|---|---|---|
| Input bytes/depth | parser limits + adversarial tests | can input exhaust memory/stack? |
| Queue/concurrency | bounded constructors + peak-in-flight tests | what happens at capacity? |
| Dependency surface | `cargo deny`, `cargo machete`, lockfile/package review | is the dependency maintained and necessary? |
| Unsafe surface | `unsafe_code = "deny"` with scoped exceptions | is each exception justified and Miri-tested? |
| Public API | API diff and consumer fixtures | was the change intentional and compatible? |
| Suppression debt | reason/expiry audit | is the exception still necessary? |
| Generated artifacts | regenerate + clean-diff check | are source and generated output synchronized? |

Do not use file length, function length, or generic complexity limits as a
substitute for identifying the actual cohesion/concurrency/resource problem.

## Suppression and exception policy

- Prefer fixing the rule violation. Suppress at the narrowest scope when the
  rule is wrong for this case or a deliberate migration needs time.
- Rust: prefer `#[expect(lint, reason = "…")]` so a stale suppression fails when
  the lint stops firing.
- TypeScript: use the established ESLint rule requiring a description for
  disable directives. `@ts-expect-error` is for a negative type test or an
  explained temporary compiler limitation, not ordinary product code cleanup.
- No repository-wide disabling of correctness, async, unsafe, or boundary rules
  to make CI green.
- A temporary exception records path, rule, reason, owner, issue, and expiry in
  the repository's chosen exception register or inline metadata tooling.
- Expired exceptions fail a scheduled debt check. The fix may be code cleanup,
  a justified policy change, or a newly reviewed bounded extension—not silent
  deletion of the expiry.
- Exceptions on security, memory safety, and data loss receive human review.

## Gate profiles

### Fast: changed code and cheap deterministic checks

Run locally before handoff and on every PR.

Rust:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
```

TypeScript/Svelte:

```bash
pnpm exec tsc --noEmit
pnpm exec eslint .
pnpm test
```

Use `svelte-check` instead of/in addition to bare `tsc` for Svelte projects.
Run the package manager's actual scripts when they define the authoritative
commands. Start with targeted tests, then run the required workspace suite.

Fast gates also include changed type-level tests, import boundaries, schema
validation tests, and generated-output freshness when relevant.

### Merge: behavior across real boundaries

- Full unit/integration suites and isolated real-database adapter tests.
- Public API/declaration consumer checks for libraries.
- Production build and package export checks.
- Dependency vulnerability/license policy where configured.
- Critical E2E paths affected by the diff.
- No unresolved failed or inconclusive required checks.

### Scheduled/deep: expensive exploration and debt

- Miri for relevant unsafe code and reduced Loom suites for custom concurrency.
- Fuzzing beyond the checked-in smoke corpus.
- Focused mutation testing of critical invariants.
- Dependency/audit updates and exception-expiry audit.
- Stress/soak tests and controlled performance regression runs.

Deep checks require an owner and actionable failure artifacts. A scheduled red
job nobody reads is not a quality gate.

### Diff-triggered: risk-specific additions

| Diff contains | Add |
|---|---|
| public Rust/TS API | API diff, consumer/type fixtures, release compatibility check |
| parser/untrusted input | malformed corpus, properties, fuzz smoke, input limits |
| lock/atomic/task lifecycle | deterministic interleaving, Miri/Loom as applicable |
| async retries/timeouts | fake-clock, cancellation, failure-injection tests |
| persistence/transaction | real adapter, rollback, concurrent claim, restart tests |
| generated code | regeneration and clean-diff check |
| dependencies/manifest | audit, unused-dependency check, feature combination tests |
| critical UI workflow | accessible browser test for the affected journey |
| optimization | before/after controlled benchmark with correctness enabled |

Missing optional tooling is **not run / not available**, never **passed**. Install
or change dependencies only under the repository's normal approval policy.

## Risk-led code review

Review the diff and surrounding contract before generic style:

1. State the intended behavior and affected public/durable interfaces.
2. Name the top three ways this change could cause incorrect results, data loss,
   security exposure, hangs, or unbounded resource use.
3. Check illegal states and transition handling against the type-driven guides.
4. Check ownership, cancellation, retry, and idempotency at every boundary.
5. Check that tests use an oracle sensitive to those failures.
6. Refute suspected findings against actual code paths; do not report guesses
   as vulnerabilities.
7. Report findings ordered by severity with file/line, failing scenario, and a
   minimal fix direction. Separate verified failures from open questions.

No findings does not mean no risk. State residual risk and missing checks.

## Exact-commit evidence

For a wiki with the code-aware `test` node type, record one execution per
`tests/` page using the existing example. The record must contain the exact
command, full commit, timestamp, exit code, and outcome.

- `passed` + `exit_code: 0` earns verification for that commit only.
- A command run before later edits does not verify the later tree.
- Cached, skipped, timed-out, and tool-missing results are not passes.
- Keep logs/reports outside concept pages; link a concise durable report if
  needed. No transcripts or large tool dumps.
- For wikis without the `test` node type, report evidence in the task/PR and do
  not invent graph nodes the ontology cannot validate.

## Adopting gates without a flag day

1. Add deny-by-default rules with no existing violations first.
2. Fix cheap current warnings that block the standard `-D warnings` gate.
3. Introduce higher-noise lints incrementally with explicit scope and rationale.
4. Never mass-suppress existing violations. Track real migration debt.
5. Make one PR's required commands match CI exactly.
6. Remove duplicate/contradictory checks once an equivalent gate is trusted.

For `thinking-os-core`, `tools/Cargo.toml` and CI are the dogfood surface. New
guides are intentionally language/tool-version aware, but each repository's own
formatter, lint configuration, and supported toolchain remain authoritative.

## Quality report format

```markdown
## Scope and risk
- Diff/commit reviewed:
- Invariants and highest-risk paths:

## Findings
- Severity, file:line, concrete failing scenario, fix direction.

## Checks
- PASS/FAIL: exact command and relevant result.
- NOT RUN: command/tool and reason.

## Residual risk
- Unverified behavior and recommended next gate.
```

Do not equate green checks with a complete review, and do not bury a required
red check beneath a summary saying "everything looks good."

## Review checklist

- Is every important architecture rule attached to a maintained detector?
- Can a lower layer import an outer layer or bypass its public surface?
- Are exceptions narrow, justified, owned, and expiring?
- Does the selected gate match the actual changed risk?
- Are missing/inconclusive checks distinguished from passes?
- Does the report lead with concrete findings and preserve residual risk?

_Provenance:_ house quality policy grounded in architecture fitness functions,
Rust/TypeScript tooling, and agent-review practice. No immutable source has been
ingested for this page yet. See [[rust-style-guide]] and [[ts-architecture]] for
the concrete baseline configurations.

Back to [[verification-patterns]] · [[library]].
