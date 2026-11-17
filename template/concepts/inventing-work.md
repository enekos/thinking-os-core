---
title: Inventing work
summary: "with no PM, work is invented from 11 signals; pick by leading/lagging × how pre-argued, not by loudness"
type: concept
updated: 2026-11-17
sources:
  - sources/2026-09-30-inventing-work.md
edges:
  - "EVIDENCED_BY 2026-09-30-inventing-work"
---

# Inventing work

Engineering-led work (a platform team, or a portfolio of side projects) has no
PM handing over a roadmap. The signals are always there; the hard part is
justifying which one to act on.

## The signals

| From | Signal | Lead/lag | Argument for free? |
|---|---|---|---|
| system | postmortems / crashes | lagging | yes |
| system | cost (bill, contracts, buy-vs-build) | lagging | yes |
| system | own toil | lagging | partly |
| users | continuous discovery (pains, indexed by frequency) | leading | no, build the spec |
| users | **overloaded use-cases** | leading | **yes, running in prod** |
| users | partner-to-prototype | leading | by building it |
| org | OKRs | lagging | yes |
| org | manager repetition (2× in a week) | ? | no, weakest |
| org | migration debris (laggards) | both | partly |
| industry | describe the existing system → RFC | n/a | no; catches expired decisions |
| industry | lag arbitrage (import the industry's evidence) | leading | reasoning yes, standing no |

## Rules worth keeping
- **Litmus for absorbing a user workaround: who else has the same problem?**
- **Every real pain already has a hack.** No hack → the pain isn't acute.
- **Build vs buy is revisitable** when scope, team, tech or market shifts.
- **Your toil ≠ your users' toil.** Yours improves unit economics; theirs, the experience.
- **The failure mode is picking the loudest signal, not running out of work.**

## Where it applies here
- **Side projects:** prefer "distance from being real" over new features. The
  overloaded use-case to look for: which of the owner's tools keep getting used
  for jobs they weren't built for? Those are worth promoting.
- **`/today`:** zaindu events (red checks, merges) are crash-style signals —
  pre-argued and lagging. They crowd out leading signals unless the briefing
  ranks them explicitly.
- **Descriptive writing** is what this wiki does: a page for a system that
  already exists is how expired decisions get noticed.

Back to the [[library]].
