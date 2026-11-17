---
name: fan
description: Design and run a task graph for a piece of work — find where it splits into pieces that never read each other's results, draw the graph, fan out only that, verify in a separate context, own the merge. May conclude the work should stay with one agent. Triggers on "/fan <work>", "fan this out", "is this work splittable", "parallelize this".
---

# /fan <work>

Design and run a task graph for the work given as the argument.

A fixed planner → coder → tester → reviewer chain is the shape the stop rule
says loses: every multi-agent configuration degrades on sequential work where
each step needs the full picture. `/fan` decides the shape first and may decide
the answer is one agent.

Read the `graph` skill's `references/task-graphs.md` before drawing anything.

**1 — Restate and scope.** What is being built, for whom, and the 2–3
assumptions you are making. If an assumption is a guess, ask now.

**2 — Find the split.** *Where does this work divide into pieces that never read
each other's results?*

- List the candidate jobs.
- For every "and then", check whether the next job reads the previous job's
  output. If not, the edge is fake — delete it.
- If nothing splits, **say so and do the work yourself in one context.** A valid,
  common outcome; report it as a finding, not a failure.

**3 — Draw it.** Nodes, real edges, which run in parallel, where the verifier
sits, where the human gate is. Name the diamond if it is one. Get approval on
the drawing.

**4 — Run it.** Fan out the independent jobs in one message so they run
concurrently. Then:

- **Verify in a separate context.** A worker never checks its own output. Give
  each verifier a different question — correct? current? does it run?
- **Own the merge.** You are the single coordinator; findings never merge
  agent-to-agent.

**5 — Guardrails.**

- One writer per file.
- A round cap on every loop.
- Max 3 spawned agents unless the user says otherwise.
- The human gate sits on irreversible edges only: push, deploy, publish,
  delete, merge. Nothing is pushed, merged or deployed without the user.
- Judge on numbers that cannot argue back — tests that ran, output that was
  diffed — never on an agent's self-report.

**6 — Report.** The graph you ran, what each node returned, what the verifiers
rejected, and the one thing you would change about the shape next time.
