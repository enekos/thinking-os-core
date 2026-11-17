---
title: TypeScript architecture patterns
summary: "TS backend + frontend defaults: feature modules, inward dependencies, use cases built in factories, ports with in-memory fakes, parse-at-the-edge, strict tsconfig, Svelte 5 on osagai; plus the mistakes agents make"
type: concept
updated: 2026-11-17
edges:
  - "APPLIES_TO osagai"
---

# TypeScript architecture patterns

Defaults for TypeScript services and web apps, written against the failure modes
agents produce. A repo's own conventions win: **match the surrounding code
first**, apply this page where the repo is silent or new. Rust counterpart:
[[rust-style-guide]]. Which stack to pick at all: [[stack-defaults]].

Priority when rules collide: **correct → secure → readable → consistent with
the codebase → fast.**

Contents: [Before "done"](#before-you-say-done) · [Backend](#backend-shape) ·
[Types](#types) · [Errors](#errors) · [Async](#async) · [Data](#data--persistence) ·
[Frontend](#frontend-svelte-5--osagai) · [Testing](#testing) · [Monorepo](#monorepo--packages) ·
[Baseline config](#baseline-config) · [Agent mistakes](#things-agents-reliably-get-wrong)

## Before you say "done"

1. `tsc --noEmit` (or `svelte-check`) is clean — zero errors, zero new `any`.
2. Lint clean, including `no-floating-promises` and import-boundary rules.
3. Tests for the changed use case pass; a bug fix ships with the test that failed.
4. No `as` cast, `!` non-null assertion or `@ts-ignore` added without a
   one-line reason in the PR (not in the code).
5. Every external input you touched is parsed by a schema at the edge.
6. No `process.env` read outside the config/factory layer.
7. The diff reads like the file around it: naming, layout, comment density.

## Backend shape

### Feature modules, layered inside

```
src/modules/<feature>/
  api/             transport: http routes, grpc handlers, queue consumers, cron — thin
  application/     use cases, dto, ports (interfaces), pure services
  domain/          entities, value objects, invariants — no I/O, no framework
  infrastructure/  adapters: repositories, http clients, queue publishers
  <feature>.factory.ts   wires the above; the ONLY place env and singletons meet
src/shared/        cross-module kernel: errors, result, ids, clock, logger port
```

- **Split by feature, then by layer.** A change to "invoices" touches one folder.
  Top-level `controllers/ services/ repositories/` scatters every feature.
- **Dependencies point inward**: `api → application → domain`;
  `infrastructure → application` (it implements ports). Domain imports nothing
  from the app. Enforce with an import-boundary lint
  (`eslint-plugin-boundaries` / `dependency-cruiser`), not good intentions.
- **Modules talk through their public surface** (`modules/x/index.ts` or a
  facade use case), never by importing another module's repository or entity.
- **Transport is thin**: parse input → call one use case → map result to the
  response. No business rule in a route handler.

### Use cases

- One operation per use case: `CreateInvoice`, `ArchiveCustomer`. A class with
  `execute(input)` or a function `createInvoice(deps)(input)` — pick what the
  repo uses.
- **Dependencies by constructor, built in a factory.** The factory reads config
  and passes plain values (limits, flags, URLs) in. Use cases and application
  utilities stay pure and testable — they never read env or `new` an adapter.
- Input and output are explicit DTO types, never ORM entities or transport
  objects.
- A use case that publishes events uses an **outbox**: write the event in the
  same transaction, a relay publishes after commit. "Save then publish" loses
  events on crash; "publish then save" announces things that never happened.

### Ports and adapters

- A port is an interface named for its **role** (`InvoiceRepository`,
  `FeedbackSink`, `Clock`, `Mailer`), declared in `application/`, implemented
  in `infrastructure/`.
- Every port has an **in-memory fake** used by tests. Fakes beat mocks: they keep
  state, so tests assert outcomes ("the invoice is archived"), not call sequences.
- Time, randomness and ids are ports too (`Clock`, `IdGenerator`) — the cheapest
  way to make tests deterministic.

### Configuration

- Parse env **once at startup** into a typed, frozen config object (schema
  validated: missing or malformed → crash at boot, not at the first request).
- Only factories read config. A `process.env.X` inside a use case is a bug:
  untestable, and read at import time it ignores per-test overrides.
- Named constants for every threshold, list and magic number; derive lists from
  metadata where possible instead of hand-writing them.

## Types

- `strict` plus the extra flags below; treat `any` as a compile error
  (`unknown` + narrowing instead).
- **Model states as discriminated unions**, not optional-field soup:
  `{ status: 'draft' } | { status: 'sent'; sentAt: Date }`. Exhaustive
  `switch` with a `never` default so a new state is a compile error everywhere.
- **Branded ids** (`type InvoiceId = string & { readonly __brand: 'InvoiceId' }`)
  when two id kinds can be swapped by accident.
- `as const` objects / string-literal unions instead of `enum`.
- `satisfies` to check a literal against a type without widening it.
- **Parse, don't validate**: external data (HTTP bodies, queue messages, env,
  `JSON.parse`, LLM output) enters as `unknown` and leaves a schema
  (zod / valibot / a generated decoder) typed. Derive the TS type from the schema,
  never maintain both.
- Generate clients from the server's contract (OpenAPI, protobuf, a typed
  contract like [[sutegi]]'s) instead of hand-writing request/response types.
- `readonly` on DTOs and config; mutate only what you own.

## Errors

- **Expected failures are values**: a use case returns
  `Result<T, DomainError>` (or throws a typed `DomainError` subclass — match the
  repo). Unexpected failures throw.
- `DomainError` carries a stable `code` (`invoice.already_sent`), never just a
  message. **Map codes to HTTP/gRPC status in one place** at the transport edge.
- Never swallow: a `catch` either handles (with a decision), wraps with context
  (`new XError('…', { cause })`), or rethrows. `catch {}` and
  `catch (e) { console.log(e) }` are bugs.
- Catch variables are `unknown`; narrow before reading `.message`.
- Don't leak internals: stack traces and SQL stay in logs, the client gets the code.

## Async

- **No floating promises** (lint-enforced). Every promise is awaited, returned,
  or explicitly handed to a supervisor with error handling.
- **Bound concurrency**: `Promise.all` over an unbounded list is a self-DoS.
  Use a limiter (`p-limit` or a small pool) for fan-out to I/O.
- **Timeouts and cancellation**: every outbound call has a timeout; thread an
  `AbortSignal` through long operations.
- **Idempotent consumers**: queue/pub-sub handlers assume at-least-once
  delivery — dedupe key or upsert, never "insert and hope".
- Don't mix `await` in loops with intended parallelism; say which one you mean.

## Data & persistence

- Repositories return **domain objects or DTOs**; ORM entities never cross into
  `api/` or responses (lazy relations and hidden columns leak).
- **Transactions are explicit**: pass a unit-of-work / transaction handle into
  the repository calls that must commit together; don't rely on ambient magic.
- Migrations are forward-only, reviewed SQL; `synchronize: true` only in
  throwaway prototypes.
- Paginate every list endpoint; cap page size server-side.
- Dates: store UTC instants (`timestamptz`); calendar dates as `YYYY-MM-DD`
  strings, never `Date` (no timezone to get wrong).
- Tests get an **isolated database** per worker/run; a shared test DB makes
  parallel runs flaky.

## Frontend: Svelte 5 + osagai

The default UI stack is SvelteKit (SPA via `adapter-static` when a Rust server
serves it) on [[osagai]]. Use the kit before writing a component.

- **Routes are thin**: load data, compose kit components, call the API module.
  Business rules live on the server.
- **One API module** (`lib/api.ts`, ideally generated from the server contract):
  base URL, auth, error decoding. Components never call `fetch` directly.
- **Reads with `query()`, writes with `task()`** from osagai: they replace the
  `busy`/`try`/`catch`/`toast` boilerplate, keep last data while reloading and
  drop stale responses. `draft()` for unsaved-changes state, `confirm()` /
  `prompt()` instead of hand-rolled dialogs, `toast` for feedback,
  `shortcuts()` for keys.
- **Shared state** in `*.svelte.ts` modules with runes (`$state`, `$derived`),
  exported as functions/classes; no global writable soup.
- **Styling through tokens**: `--o-*` variables from `osagai/styles/theme.css`;
  app CSS adds app-only variables. No hex colours in components; dark mode comes
  free.
- Accessibility is the kit's job — don't re-implement focus traps, menus or
  comboboxes; extend the kit (upstream) when something is missing.
- `svelte-check` 0 errors / 0 warnings is part of "done".

## Testing

| Layer | What | With |
|---|---|---|
| Domain | invariants, pure functions | plain unit tests, no setup |
| Use case | behaviour through ports | in-memory fakes, fixed `Clock` |
| Adapter | SQL, HTTP clients | real DB (isolated), recorded/fake server |
| Transport | parse + status mapping | a few request tests per route |
| E2E | the critical user path only | browser driver, seeded data |

- Test behaviour, not implementation: assert state and outputs, not that
  `repo.save` was called once.
- Test names say the rule (`archives an invoice only once`), no ticket ids.
- LLM features need evals, not just tests → [[eval-methods]].

## Monorepo & packages

- pnpm workspaces; one `tsconfig.base.json`, per-package `tsconfig` extends it.
- A package exposes its API via `package.json` `exports`; deep imports into
  another package's `src/` are banned.
- Avoid barrel files that re-export everything — they create import cycles and
  slow tooling. Export the public surface explicitly.
- Shared packages (UI kit, contracts) are consumed from source via aliases during
  development, versioned when published.

## Baseline config

```jsonc
// tsconfig.base.json — compilerOptions
"strict": true,
"noUncheckedIndexedAccess": true,
"exactOptionalPropertyTypes": true,
"noImplicitOverride": true,
"noFallthroughCasesInSwitch": true,
"verbatimModuleSyntax": true,
"isolatedModules": true,
"useUnknownInCatchVariables": true,
"skipLibCheck": true
```

```js
// eslint (typescript-eslint, type-aware)
'@typescript-eslint/no-floating-promises': 'error',
'@typescript-eslint/no-misused-promises': 'error',
'@typescript-eslint/no-explicit-any': 'error',
'@typescript-eslint/no-non-null-assertion': 'error',
'@typescript-eslint/switch-exhaustiveness-check': 'error',
'@typescript-eslint/consistent-type-imports': 'error',
'@typescript-eslint/only-throw-error': 'error',
// + an import-boundary rule set for the layer graph above
```

## Things agents reliably get wrong

1. **`as` to silence the compiler** — the cast hides the bug the error was about.
2. **`any` creeping in** through untyped `JSON.parse`, `catch (e: any)`, or
   `Record<string, any>`.
3. **Floating promises** — a missing `await` in a handler; the error vanishes.
4. **`process.env` at import time** deep in a module.
5. **ORM entities returned from routes** — leaks columns, triggers lazy loads.
6. **Business logic in the route handler** because it was the open file.
7. **Mocks asserting call order** instead of fakes asserting state.
8. **Unbounded `Promise.all`** over user-sized input.
9. **`catch` that logs and continues** — the caller believes it worked.
10. **`Date` for calendar dates** — off-by-one across timezones.
11. **Hand-written API types** that drift from the server.
12. **New UI primitives** (a button, a modal) when the kit has one.
13. **Barrel imports** creating cycles between modules.
14. **Duplicating a schema and a type** instead of deriving one from the other.
15. **Comments narrating the code** — in a no-comments codebase the
    explanation goes in the PR description.

_Provenance:_ the owner's practice across TypeScript services and Svelte apps;
no external source ingested yet. When one is, add it under `sources/` and an
`EVIDENCED_BY` edge here.

Back to the [[library]].
