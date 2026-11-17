---
title: Default stack — sutegi server, SvelteKit + osagai UI
summary: "accepted: new apps start on sutegi (Rust, std-only) serving a SvelteKit SPA on osagai; deviations need their own decision"
type: decision
status: accepted
updated: 2026-11-17
edges:
  - "DECIDES stack-defaults"
  - "AFFECTS sutegi"
  - "AFFECTS osagai"
---

# Default stack — sutegi server, SvelteKit + osagai UI

**Decision.** Every new app or service starts from [[stack-defaults]]: one
[[sutegi]] binary (SQLite, Postgres when multi-pod) serving a SvelteKit SPA
built on [[osagai]].

**Why.**
- One small binary, no async runtime or third-party dependency tree to audit.
- Every app is agent-drivable through sutegi's `/__tools` without extra code.
- osagai already solved focus, loading, errors, toasts and theming; apps stop
  re-solving them.
- Shared patterns mean shared gotchas: a fix in the library fixes every app.

**Consequences.**
- Gaps are fixed upstream in sutegi/osagai, not worked around per app.
- A non-default choice is a new decision that `DECIDES` the project and
  `AFFECTS stack-defaults`.
