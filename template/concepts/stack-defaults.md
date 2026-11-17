---
title: Stack defaults
summary: "what to build new things on: sutegi (Rust, std-only) for the server, SvelteKit + osagai for the UI, SQLite until you need pods; when to deviate"
type: concept
updated: 2026-11-17
edges:
  - "APPLIES_TO sutegi"
  - "APPLIES_TO osagai"
---

# Stack defaults

Read this before scaffolding anything new. The defaults exist so that every
app shares one set of patterns, one set of gotchas and one upgrade path; the
cost of a non-default choice is paid on every future change, so deviate on
purpose and write a decision page when you do.

## The default app

```
one Rust binary (sutegi)  ──serves──▶  SvelteKit SPA (adapter-static) on osagai
        │
   SQLite (single node)  →  Postgres when you need more than one pod
```

| Layer | Default | Page |
|---|---|---|
| HTTP server, routing, auth, jobs, mail, storage | [[sutegi]] | [[rust-style-guide]] |
| Data | sutegi ORM on SQLite (`features = ["sqlite"]`); `postgres` for multi-pod | [[sutegi]] |
| Background work | sutegi `queue` on the same backend — no separate broker | [[sutegi]] |
| Web UI | SvelteKit 5 + TypeScript, `adapter-static`, served by the binary | [[ts-architecture]] |
| Components | [[osagai]] — kit first, never hand-rolled primitives | [[osagai]] |
| API types | generated from the server contract, never hand-written | [[ts-architecture]] |
| Agent access | sutegi `/__tools` — every app is drivable by an agent for free | [[sutegi]] |
| LLM features | evals before launch | [[eval-methods]] |

Why: one small binary to deploy, no async runtime or dependency tree to audit,
an agent surface on every app, and a UI kit that already solved focus,
loading, errors and theming.

## Scaffold

```sh
cargo install sutegi-cli            # the `sutegi` command
sutegi new my-app                   # conventional layout
sutegi make:model Thing && sutegi make:route things
pnpm dlx sv create web              # SvelteKit SPA in web/, then:
pnpm -C web add @enekos/osagai
```

Then: a project page in `projects/` with `path:` set, a decision page if
anything deviates from this table.

## When to deviate

| Situation | Instead | Why |
|---|---|---|
| Edge/serverless, request-scoped, global | Rust on Workers → [[rust-on-workers]] | sutegi assumes a long-lived process |
| Heavy async fan-out (thousands of concurrent outbound calls) | tokio + axum | sutegi is thread-pool, no async runtime |
| Existing TS service or team codebase | stay in TS → [[ts-architecture]] | match the codebase beats the default |
| Content site, no app state | static site generator | no server needed |
| Needs a crate that drags tokio in | isolate it in a worker process, keep the app std | keeps the binary and audit surface small |

## Rules

- **Pin versions** (`sutegi = "0.14"`, `@enekos/osagai@^0.9`) and read the
  changelog before bumping — security releases can invalidate sessions.
- **Fix upstream.** A sutegi or osagai gap found while building an app is fixed
  in the library (PR there), not patched around in the app.
- **Record deviations** as a decision page that `DECIDES` the project and
  `AFFECTS stack-defaults`; when a deviation repeats, revise this page.

Back to the [[library]].
