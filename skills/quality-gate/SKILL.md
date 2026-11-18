---
name: quality-gate
description: Risk-led code review and verification for a repository diff — classify the change, load the relevant Rust/TypeScript design patterns, run fast/merge/deep quality gates, distinguish missing checks from passes, and record exact-commit test evidence. Triggers on "/quality-gate", "review this diff", "run code quality checks", "verify this fix", "which tests does this change need".
---

# /quality-gate [fast|merge|deep] [repo] [task]

Apply engineering-library invariants to code. Default profile is `fast`; default
repo is the current repository. `task` is an optional existing wiki task slug
used for exact-commit evidence. This skill reviews and verifies; it does not
rewrite the diff unless the user also asks for fixes.

`/lint` checks wiki health. This skill checks product code.

## 1. Locate the repository and library

From the target repo, inspect its `AGENTS.md`/`CLAUDE.md`, status, manifests,
formatter/linter configuration, test scripts, and CI. Repository instructions
and existing commands win over generic examples.

Find the engineering library at `$THINKING_OS_ROOT/concepts`, then
`~/thinking-os/concepts`, then the core repo's `template/concepts`. The skill
is normally linked as `~/.claude/skills/quality-gate`:

```bash
CORE=$(cd "$(dirname "$(readlink -f ~/.claude/skills/quality-gate/SKILL.md)")/../.." 2>/dev/null && pwd)
```

Read only what applies:

| Change | Required pages |
|---|---|
| any Rust | `rust-style-guide` |
| Rust public types/state/API | `rust-type-driven-design` |
| Rust resources/async/threads | `rust-resource-and-concurrency-patterns` |
| any TypeScript/Svelte | `ts-architecture` |
| TS domain/state/public types | `typescript-domain-patterns` |
| TS async/retry/cancellation | `typescript-async-and-resilience-patterns` |
| tests or a bug fix | `verification-patterns` |
| architecture/CI/review policy | `architecture-fitness-and-quality-gates` |

## 2. Bound the diff and name the risks

- Inspect `git status --short`, the working/staged diff, and untracked files.
- For a PR/branch, use its actual base ref rather than guessing `main`.
- Never overwrite or discard unrelated user changes. A dirty tree is valid to
  check, but `HEAD` does not identify uncommitted edits.
- State intended behavior, affected public/durable interfaces, and the top
  three concrete failure risks. Stop at three; use them to select checks.
- If the diff is empty, review the requested commit/range or report no code to
  check. Do not manufacture findings.

## 3. Select the smallest sufficient gate

Always run the repository's required formatter, compiler/linter, and targeted
tests for the changed code unless the user explicitly requests review-only.
Start specific, then broaden when those pass.

| Diff signal | Add to the selected profile |
|---|---|
| public API/types | consumer fixtures, positive/negative compile tests, API diff |
| bug fix | regression test shown to fail without the fix where feasible |
| parser/untrusted input | malformed cases, properties/fuzz smoke, size/depth limits |
| persistence | real adapter, transaction/rollback, concurrent and restart cases |
| locks/atomics/lifecycle | deterministic interleavings; Miri/Loom where applicable |
| promises/retries/timeouts | fake time, controlled settlement order, cancellation injection |
| dependencies/manifests | lockfile review, audit/unused checks when configured |
| generated output | regenerate and require a clean semantic diff |
| critical UI flow | focused accessible browser/system test |
| performance claim | controlled before/after benchmark plus correctness |

Profiles:

- **fast** — formatting, strict lint/type-check, unit/type-level and targeted
  integration tests, import boundaries.
- **merge** — fast plus full required integration/build/API/dependency checks
  and affected critical E2E paths.
- **deep** — merge plus explicitly relevant Miri/Loom, fuzz exploration,
  focused mutation, stress/soak, performance, or exception-debt checks. Deep
  does not mean running every expensive tool indiscriminately.

Check tool availability before execution. Missing optional tools are `NOT RUN`;
never report them as passed or install/change dependencies without following
the user's approval environment.

## 4. Review while checks run

- Trace the concrete failing scenario before reporting it. Refute false
  positives against call sites and configuration.
- Look for invalid states, casts/unwraps without a nearby proof, boundary bypass,
  unbounded work, ignored errors, cancellation gaps, duplicate effects, weak
  test oracles, and unjustified suppressions.
- Do not turn personal style preferences into findings or propose a new pattern
  when the surrounding simple code is correct.
- Do not fix unrelated failures. Separate pre-existing failures from those
  introduced by this diff using evidence.

## 5. Record evidence only when it is exact

If the wiki ontology defines the `test` node type, a real task slug was supplied,
and the target worktree is clean at a full commit SHA, create one `tests/` page
per executed command by copying `tests/_example.md`. Use that exact SHA, command,
UTC timestamp, status, and exit code. Never edit the underscore example.

- Only `passed` + exit code 0 earns `VERIFIES` for that commit and task.
- Failed, timed-out, skipped, tool-missing, and inconclusive runs are not passes.
- A dirty working tree has no exact commit: include commands/results in the
  report but do not create a verification page for `HEAD`.
- If no task or code-aware ontology exists, return the evidence in the report;
  do not invent graph records.
- If the change clears the exemplar rubric in the graph skill's
  `references/code-memory.md`, say so in the report and suggest the
  `exemplar:` block. Do not add it during review: nominate after merge,
  naming the SHA that landed.
- After recording valid pages, run `sare build local` and inspect graph errors.

## 6. Report once, findings first

```markdown
## Scope and risk
- Commit/diff and intended behavior:
- Invariants and highest-risk paths:

## Findings
- Severity — file:line — concrete failing scenario — minimal fix direction.
- Pre-existing failures, separately:

## Checks
- PASS/FAIL — exact command — concise result.
- NOT RUN — command/tool — reason.

## Evidence and residual risk
- Test pages written, if any:
- Unverified behavior and recommended next gate:
```

Lead with findings, not a checklist recap. If there are no findings, say so and
still preserve failed/missing checks and residual risk. Green checks do not
cancel an unresolved correctness finding.
