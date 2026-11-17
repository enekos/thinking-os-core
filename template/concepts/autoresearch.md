---
title: autoresearch
summary: "metric-driven agent loop: hypothesis → change → benchmark → keep or revert, logged to an append-only JSONL; plus a read-only discovery variant"
type: concept
updated: 2026-11-17
---

# autoresearch

An agent loop for **iterative, metric-driven optimisation** of a defined surface.
The agent proposes a hypothesis, changes the code, runs a benchmark, parses
`METRIC` lines, keeps or reverts on the primary metric, and logs the run. The
output is a durable record of what worked, what didn't, and why.

## The five-file contract

| File | Purpose |
|---|---|
| `autoresearch.md` | Objective, primary + secondary metrics, workloads, scope, rules. The fixed contract. |
| `autoresearch.ideas.md` | Candidate hypotheses, baseline analysis, deferred items. |
| `autoresearch.jsonl` | Append-only experiment log, one object per run. Git-ignored session state. |
| `autoresearch.sh` (or equivalent runner) | Runs the bench, emits `METRIC key=value` lines. |
| bench harness in the source tree | The benchmarks. Owned by the loop, not the regular test suite. |

## JSONL shape
First line is config; every later line is one experiment:

```json
{"type":"config","name":"Optimise engine execution","metricName":"total_µs","metricUnit":"µs","bestDirection":"lower"}
{"run":12,"commit":"ad55f8e","metric":0.04,"metrics":{"mixed_total_µs":8},"status":"keep","description":"ASCII fast path for token length","timestamp":1778460530795,"segment":3,"confidence":null,"asi":{"hypothesis":"…","next_action_hint":"…"}}
```

- `metric` — primary scalar; `bestDirection` lower or higher.
- `metrics` — secondaries (watch for trade-offs).
- `status` — `keep | discard | crash`. Discards roll the workspace back via git.
- `confidence` — from variance; decides when to stop.
- `asi.hypothesis` / `asi.next_action_hint` — stated reasoning, so a new session resumes mid-loop.

## Universal rules
1. **Don't overfit the benchmark.** A 2% win that adds risk in unmeasured paths is a revert.
2. **Don't cheat.** No skipped work, hard-coded outputs, or removed computation under the bench.
3. **Preserve correctness.** All existing tests still pass.
4. **Stay in scope.** In-scope files are open; public API and unrelated code are read-only.

## Why it's worth it
- **Memory across sessions.** The JSONL is the world model; nothing gets re-derived.
- **Negative results locked in.** "Tried X, it regressed" stops future re-attempts.
- **Composes with evals.** Over an eval harness the loop tunes prompts/schemas/weights
  against a quality metric (F1, AUC, judge agreement) instead of latency → [[eval-methods]].

## Observed failure: loops go dormant
Loops run in bursts (7–100 runs) and then stop — the surface hits a local floor,
or the kickoff had no recurring trigger. A loop needs a schedule or an owner, or
it becomes a file nobody reads.

## v2 — the discovery loop
The loop above is **optimisation**: one agent, one scalar, serial because it
mutates a checkout. The orthogonal branch is **discovery** — searching for
unknown security defects or perf bottlenecks:
- **No metric to hill-climb**, so the keep/discard oracle becomes **adversarial
  refutation**: each finding goes to an independent verifier prompted to
  disprove it.
- **A finding counts only with a failing test** (or a reproducible measurement).
- **Read-only**, so it parallelises: a fleet of agents across code areas × lenses.
- **Scheduled**, which answers the dormancy problem.
- **Calibrate severity separately.** Fleets find real code reliably but
  over-rate how much it matters; most "high" findings land at medium after
  verification. Scope the sweep to deployed code first — prototypes burn
  verifier budget.
- Confirmed bottlenecks hand back into v1 contracts: discovery manufactures
  the optimisation loops.

See also: [[augmented-coding-patterns]].

Back to the [[library]].
