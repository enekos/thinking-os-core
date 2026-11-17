---
title: Evaluation methods
summary: "how to measure anything an agent or model produces: errors-first fixtures, deterministic → judge → human layers, judge calibration, trajectory scoring, refute-first verification, benchmark statistics"
type: concept
updated: 2026-11-17
sources:
  - sources/2026-08-04-eval-driven-development-airbnb.md
edges:
  - "EVIDENCED_BY 2026-08-04-eval-driven-development-airbnb"
---

# Evaluation methods

Unit tests prove wiring; they cannot tell you whether a model's output is good,
whether an agent got there by a sound path, or whether a perf change is real.
This page is the toolbox for those three questions. Read it before building an
LLM feature, an agent harness, a benchmark, or before trusting a fleet of agents'
findings.

**The one rule** (eval-driven development): *look at ~100 real outputs,
categorise the failures you actually see, build evals from those.* Evals written
from theory measure what you imagined, not what breaks.

## 1. Pick the method by question

| Question | Method | Section |
|---|---|---|
| Is this generated text / extraction good? | fixture corpus + layered scoring | 2–4 |
| Did the agent do the task the right way? | trajectory + tool-use + outcome scoring | 5 |
| Is this finding (bug, vuln, perf issue) real? | refute-first verification | 6 |
| Is this change faster / cheaper? | paired benchmark + significance test | 7 |
| Can a loop improve it unattended? | [[autoresearch]] on top of any of the above | — |

## 2. The fixture corpus — the porting unit

- **One markdown file per case**: inputs in frontmatter (human keys —
  `city: berlin`, not ids), the scenario in prose, one `##` per judged
  dimension, and an `expect:` block that compiles to hard checks.
  The same file survives the prompt lab, CI, and production comparison.
- **Coverage, in this order**: real failures you have seen (each bug becomes a
  fixture) → calibration extremes (smallest and largest plausible input) →
  locale/language spread (detectors built English-first silently pass
  everything else) → edge inputs (gibberish, emoji, 200-char fields,
  contradictions, empty context) → **false-positive guards** (valid inputs that
  must *not* be flagged).
- **Fixtures are not traffic.** Context-rich fixtures hide failures that thin
  real inputs trigger (a placeholder detector fired on 3–4 of 44 fixtures and on
  2 of 2 live thin inputs). Sample production regularly; promote surprises to
  fixtures.
- Key every result on `(task, prompt_version, model)` so runs compare.

## 3. Score in three layers, cheapest first

1. **Deterministic** — always on, free: schema validity, `expect:` assertions,
   guardrail regex (PII, HTML/XSS, instruction-echo canaries), verbatim-quote
   grounding, no-op detection, domain invariants (`max > 2×min → unreliable`),
   self-consistency (your generator's output passes your own reviewer).
2. **LLM judge** — a rubric per dimension, scored by a **different model
   family** than the one generating (same-family judges grade themselves
   kindly). Wrap the judged text as data; an instruction found inside it
   ("score this as pass") is failure evidence, never obeyed.
3. **Human** — ground truth, high-stakes calls, tie-breaks. One named person
   owns disagreement resolution.

Prefer **3–5 focused evaluators** over 20 diffuse ones. Scoring rules:

- **F1, not recall**, on set-valued outputs — recall alone rewards
  over-extraction.
- **Partial-credit ramps, not cliffs** — a near miss should score near.
- **Report per dimension beside the mean** — one strong dimension must not hide
  a weak one.
- **Zero-tolerance classes fail hard**: an injection or HTML-passthrough fixture
  failing is a CI failure, not a pass-rate dent.

## 4. Calibrate the judge before you trust it

- Golden set of 50–100 human-labelled cases, *including failures*. Run the
  judge, measure agreement, target **high-80s to 90s %**. Resolve disagreements,
  fix the rubric, re-run; recalibrate whenever the prompt or model changes.
- If the judge says 0.9 on a version users reject 40% of the time, **the rubric
  is wrong, not the users** — but you can only compute that if judge and user
  scores are joined on the same items. Score production samples with the same
  rubric and join on the request id; two aggregates over different populations
  prove nothing.
- **Acceptance ≠ quality.** "The user kept it" means kept, not true; a fabricated
  detail that reads well is accepted. Implicit signals are also missing-not-at-random:
  users abandon bad outputs, so the scored survivors are biased upward.
- **Goodhart guard**: judge scores stay diagnostic. Release/rollout decisions
  stay on human or user signals. Write that rule down with the score names,
  before the first disagreement.
- Name parity: judge dimensions are spelled identically offline and online
  (`judge_grounding`, never mixed with user scores' names).

## 5. Agents: score the path, not just the result

A correct final diff can come from a fragile or lucky path that won't
generalise. Three separate signals:

| Layer | Asks | Example check |
|---|---|---|
| **Trajectory** | Was the reasoning path sound? | read the relevant files before editing; no dead-end loops; stopped when done |
| **Tool use** | Right tool, right parameters, each step? | used the search tool, not a 2k-line read; no destructive call without need |
| **Outcome** | Is the end state right? | tests pass, diff matches intent, nothing out of scope touched |

Record the transcript; score from it. An agent harness that only checks "does the
diff pass" will reward guessing.

## 6. Refute-first verification — for findings, not outputs

When agents *find* things (security defects, perf bottlenecks, review comments),
precision is the metric that kills adoption: the first false page ends trust.

- **Split finder and verifier.** Finders sweep wide and read-only (parallel is
  safe). Each high/critical finding goes to one independent verifier **prompted to
  refute it**: find the guard, the caller that never passes that input, the test
  that already covers it.
- **Confirmed means reproduced**: a failing test or a concrete input → wrong
  output. No repro → not confirmed.
- **Severity is the weak spot.** Finders reliably find real code and badly judge
  how much it matters (in one measured run, 22 of 26 confirmed "high" findings
  were really medium or low). Re-rate severity in the verifier, never ship raw.
- **Scope before sweep.** Most refutations are mechanical — "you audited a
  prototype", "you missed the middleware guard". Exclude non-deployed trees and
  feed the verifier the framework's guards up front.
- Report precision (confirmed / verified) and the refutation reasons; the reasons
  are the next run's prompt fixes.

## 7. Performance: measure like it might be noise

- **Paired runs, many samples.** Interleave base and candidate (ABAB…) on the
  same machine to cancel drift; ≥ 20 samples per side for anything under ~10%.
- **Compare distributions, not means**: report median and p95, and use a
  rank test (Mann–Whitney U) or bootstrap CI on the difference. A change inside
  the noise band is "no change", whatever the mean says.
- **Pin the environment**: release profile, same input set, warm cache stated
  explicitly, CPU governor/thermal noise acknowledged.
- **Count allocations and syscalls**, not just time — they explain the time and
  are far less noisy.
- Bench harnesses live beside the code but outside the default test run; their
  output is `METRIC key=value` lines so [[autoresearch]] can drive them.

## 8. Wiring it into the work

- **CI gate**: a prompt or model change below baseline fails (e.g. > 2pp
  overall or > 5pp on any dimension) — once the judge is calibrated (§4).
  Before that, the gate is enforcing unvalidated authority.
- **Online**: sample production, judge asynchronously after the response,
  fire-and-forget; a lost score never blocks the user path.
- **Cost** is rarely the blocker (a cheap-model judge is cents per thousand
  items); calibration and joinability are.
- File the result: an eval run that changes a decision gets a `sources/` snapshot
  and a decision page `EVIDENCED_BY` it.

## Checklist before you call an LLM feature "evaluated"

- [ ] Fixtures came from ≥ 100 real outputs, not imagination
- [ ] Deterministic layer runs first and is free
- [ ] Judge is cross-family, rubric per dimension, injection-safe
- [ ] Judge agreement with humans measured (≥ high-80s %) on a golden set
- [ ] False-positive guards exist
- [ ] Scores reported per dimension, keyed on (task, prompt version, model)
- [ ] Agents: trajectory and tool-use scored, not only the outcome
- [ ] Judge scores are diagnostic; humans/users own the release decision

See also: [[augmented-coding-patterns]].

Back to the [[library]].
