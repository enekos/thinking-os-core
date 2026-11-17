---
title: "A Staff Engineer's Guide to Inventing Work — Sujith Jay Nair"
summary: "essay: 11 signals for engineering-led teams to decide what to build, ranked by argument-for-free × leading/lagging → [[inventing-work]]"
type: source
kind: essay
date: 2026-09-22
url: https://sujithjay.com/inventing-work
author: Sujith Jay Nair
updated: 2026-11-17
---

# A Staff Engineer's Guide to Inventing Work (source)

`https://sujithjay.com/inventing-work` — Sujith Jay Nair, 2026-09-22. Ingested
2026-09-30. This is a structured distillation, not the full text; the article
lives at the URL.

## Premise

Platform teams are engineering-led: no PM, no revenue line, no market. Work does
not exist until an engineer invents it. The signals are already out there, from
four directions.

## The eleven signals

**System**
1. **Crash-led discovery** — postmortems say what to fix/replace; patterns
   *across* postmortems are rarely mined. Biased to loudest + most recent;
   maximally lagging.
2. **Cost** — the cloud bill substitutes for a revenue north star. Go past query
   tuning: read the P/L, vendor contracts, line items. Buy-vs-build is not a
   one-time decision; revisit in both directions.
3. **Own toil** — never prioritized. Trap: your toil ≠ your users' toil (unit
   economics vs user experience).

**Users**
4. **Continuous discovery** — talk to N users per period: walk through the last
   task, top three pains, what solving them means, who else has it. Every real
   pain already has a hack; no hack → not acute. Push back on proposed
   solutions. Index pains by frequency. Problem space only.
5. **Overloaded use-cases** (author's favourite) — users pressing the platform
   into jobs it wasn't designed for are prototypes built for you. Litmus: who
   else has the same problem?
6. **Partner-to-prototype** — the same, arranged deliberately; a joint
   exploration, not a promise.

**Org**
7. **OKRs** — work already invented.
8. **Manager repetition** — heard twice in a week from up the chain → an
   unaddressed concern. Weakest signal: far from users, HiPPO risk.
9. **Migration debris** — the laggards in a migration's adoption curve show where
   the "median" solution fails.

**Industry**
10. **Descriptive → prescriptive writing** — write a design doc for the system
    that exists, compare to state of the art; describing surfaces decisions that
    have silently expired.
11. **Lag is your arbitrage** — domains swing bundling ↔ unbundling (warehouse →
    lake → lakehouse; monolith → microservices → modular monolith). Internal
    platforms follow with a delay, so you import the reasoning and evidence
    (public postmortems, benchmarks, abandoned positions) for free. Failure mode:
    too late, the straggler set.

## Which signal, when

Two axes: **how much of the argument the signal hands you for free**, and
**leading vs lagging**.

- Pre-argued, cheap, lagging: postmortems, cost centres, OKRs.
- Argument you must construct: lag arbitrage (strongest reasoning, weakest
  standing), manager repetition (no evidence, some standing), continuous
  discovery (pains, but specs are on you).
- Middle: overloaded use-cases — leading, and the argument is already running in
  production. Partner-to-prototype buys the same evidence by building it.
  Migration debris is lagging for the last migration, leading for the next.
  Descriptive writing: no argument, no urgency, best at catching expired
  decisions.

## Closing

Not a shortage problem. The failure mode is a backlog assembled from the loudest
signals — the crash, the skip-level. Inventing work is being able to say why this
signal and not the other ten.
