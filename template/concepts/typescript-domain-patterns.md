---
title: TypeScript domain design patterns
summary: "modern TypeScript patterns for branded identifiers, discriminated states, exhaustive transitions, validated boundaries, typed errors, small ports, immutable messages, and executable type-level tests"
type: concept
updated: 2026-11-17
edges:
  - "APPLIES_TO osagai"
---

# TypeScript domain design patterns

Use TypeScript's structural type system to make domain rules visible and illegal
states expensive to express. Do not translate Java class hierarchies by default.
The surrounding feature/layer architecture is in [[ts-architecture]]; tests are
in [[verification-patterns]].

## Choose the smallest enforcement mechanism

| Rule | Pattern | Verification |
|---|---|---|
| IDs of different entities cannot be swapped | branded ID + validating factory | negative type test + parser tests |
| Exactly one state is active | discriminated union | exhaustive `switch` and transition table |
| A lookup contains every declared key | `satisfies Record<Key, Value>` | compiler |
| Input is trustworthy after parsing | `unknown` → schema → typed value | valid/invalid generative tests |
| Expected failures preserve their category | typed `Result` or typed error union | exhaustive error tests |
| A side effect can vary without changing a use case | narrow function/interface port | fake conformance suite |
| A transition is a pure decision | command/event + pure reducer | model/property tests |

Types disappear at runtime. Every value arriving from the network, storage,
environment, or an LLM still needs runtime validation.

## Branded identifier with validated construction

**Use when.** `InvoiceId` and `CustomerId` share a representation but are not
interchangeable.

```ts
declare const invoiceIdBrand: unique symbol;

export type InvoiceId = string & {
  readonly [invoiceIdBrand]: 'InvoiceId';
};

export function parseInvoiceId(raw: unknown): InvoiceId {
  if (typeof raw !== 'string' || !/^[a-z][a-z0-9-]{0,63}$/.test(raw)) {
    throw new Error('invalid invoice id');
  }
  return raw as InvoiceId;
}
```

The assertion is concentrated in the validating factory, where the runtime
check establishes the invariant. Do not export a general `asInvoiceId` cast.

- Keep brand symbols private when arbitrary downstream branding would defeat
  the boundary.
- Prefer deriving the branded type from the existing schema library when that
  keeps one validator and one inferred type.
- A brand distinguishes compile-time types; it does not validate a string.
- At storage boundaries, wrap and unwrap deliberately. Do not leak a database
  driver's branded type into the domain.

**Checks.** Test empty, malformed, oversized, non-string, and boundary values.
Add negative type tests for passing `CustomerId` or raw `string` to an operation
requiring `InvoiceId`.

**Avoid when.** There is one ID kind in a tiny local module and construction
has no invariant. Brands add conversion friction without adding runtime safety.

## Discriminated union state

**Use when.** State-specific fields differ or a transition is legal only in
certain states.

```ts
type Publication =
  | { readonly status: 'draft' }
  | { readonly status: 'ready'; readonly reviewedBy: string }
  | {
      readonly status: 'published';
      readonly reviewedBy: string;
      readonly publishedAt: Date;
    };

function assertNever(value: never): never {
  throw new Error(`unhandled publication: ${JSON.stringify(value)}`);
}

function canPublish(publication: Publication): boolean {
  switch (publication.status) {
    case 'draft':
      return false;
    case 'ready':
      return true;
    case 'published':
      return false;
    default:
      return assertNever(publication);
  }
}
```

- Reuse common fields through a base object type if useful, but keep the
  discriminant literal and unique.
- Do not add a broad `PublicationBase` cast to silence an exhaustiveness error.
  The error identifies a state whose behavior has not been decided.
- Store external timestamps according to the persistence policy; validate them
  before constructing this domain state.

**Checks.** `@typescript-eslint/switch-exhaustiveness-check` with type-aware
linting. A model test enumerates states and events and compares legal
transitions with the implementation.

**Anti-pattern.** `{ draft?: boolean; reviewedBy?: string; publishedAt?: Date }`
allows contradictory combinations and forces every consumer to recheck them.

## Exhaustive policy tables with `satisfies`

**Use when.** Metadata or policy must have exactly one entry per key while
preserving useful literal inference.

```ts
const retryPolicy = {
  validation: { attempts: 0 },
  rateLimit: { attempts: 4 },
  unavailable: { attempts: 4 },
} satisfies Record<FailureKind, { attempts: number }>;
```

`satisfies` checks without widening every property to its annotation. A missing
key fails at the table. It does not reject every excess property in nested
values, so tests still own behavioral completeness.

Prefer `as const` objects and derived string unions over TypeScript `enum` when
the repo has no established enum convention. Numeric enums and cross-file enum
evolution often obscure the actual wire/domain value.

## Parse at the boundary, not throughout the domain

**Use when.** Data crosses any untyped boundary: HTTP, queue, local storage,
`JSON.parse`, environment, CLI, database driver, or model output.

**Invariant.** Domain constructors receive already validated values.

```ts
function decodeCommand(schema: CommandSchema, raw: unknown): Command {
  const result = schema.safeParse(raw);
  if (!result.success) {
    throw new InvalidCommand(result.error);
  }
  return result.data;
}
```

- `JSON.parse` returns `any` in the standard library. Assign its result to
  `unknown` immediately and pass it to a decoder.
- Infer the static type from the schema where practical; do not maintain a
  second manually synchronized interface.
- Validation returns a typed value; a boolean `validate(value)` followed by a
  cast leaves every future caller responsible for repeating the ritual.
- Decide whether unknown object keys are stripped, preserved, or rejected at
  each boundary. Do not inherit a schema library default accidentally.

**Checks.** Generate valid and malformed inputs. Include missing fields, wrong
primitive types, extra keys, non-finite numbers where JSON tooling permits them,
and size/depth limits. Property-test successful decode → encode → decode where
the representation has a round-trip invariant.

## Typed expected failures

**Use when.** A caller must make a different decision for each expected failure.

```ts
type PublishError =
  | { readonly code: 'not_reviewed' }
  | { readonly code: 'already_published' };

type Result<T, E> =
  | { readonly ok: true; readonly value: T }
  | { readonly ok: false; readonly error: E };

function publish(publication: Publication): Result<Publication, PublishError> {
  if (publication.status === 'draft') return { ok: false, error: { code: 'not_reviewed' } };
  if (publication.status === 'published') {
    return { ok: false, error: { code: 'already_published' } };
  }
  return { ok: true, value: publication };
}
```

Choose `Result` or typed exceptions according to the repository; do not mix
styles arbitrarily inside one use case.

- Error codes are stable machine contracts; messages are for people.
- Adapter failures need context and `cause`, not a leak of SQL/stacks into the
  domain result.
- A thrown exception should mean the repository's exception convention or an
  unexpected failure, not an invisible alternative return channel.
- At the transport boundary, map the complete expected error union to statuses
  in one place.

**Checks.** Exhaustively handle error codes in tests and transport mapping.
Use type-aware `only-throw-error`; test that unexpected failures retain `cause`.

## Narrow ports and explicit factories

**Use when.** A use case needs a replaceable side effect or deterministic test
double.

```ts
interface InvoiceStore {
  byId(id: InvoiceId): Promise<Invoice | undefined>;
  save(invoice: Invoice): Promise<void>;
}

interface Clock {
  now(): Date;
}

export function makeArchiveInvoice(deps: { store: InvoiceStore; clock: Clock }) {
  return async function archiveInvoice(id: InvoiceId): Promise<Result<Invoice, ArchiveError>> {
    const invoice = await deps.store.byId(id);
    if (!invoice) return { ok: false, error: { code: 'not_found' } };
    return archive(invoice, deps.clock.now());
  };
}
```

- Declare the port next to its consumer, not in a global `interfaces/` folder.
- Keep the interface minimal. A `Database` port exposing every query is an
  adapter leak, not dependency inversion.
- The composition root/factory creates adapters and passes plain config values.
  Use cases do not read `process.env` or instantiate global singletons.
- A single async function dependency is often clearer than a one-method class.
- Every stateful fake should obey the same contract as the real adapter.

**Checks.** Run one reusable conformance suite against in-memory and production
adapters where feasible: not-found behavior, ordering, atomicity, duplicate
writes, and error mapping. Use-case tests assert observable state, not mock call
order.

## Pure decisions with commands and events

**Use when.** Business behavior needs auditability, replay, undo, or a clear
separation between deciding and performing effects.

```ts
type Command =
  | { readonly type: 'review'; readonly reviewer: string }
  | { readonly type: 'publish'; readonly at: Date };

type DomainEvent =
  | { readonly type: 'reviewed'; readonly reviewer: string }
  | { readonly type: 'published'; readonly at: Date };
```

- A command requests a transition and may be rejected. An event records a
  decision that occurred; naming must preserve that distinction.
- The pure reducer validates state and returns events/result. The application
  layer loads state, persists atomically, and performs effects afterward.
- `readonly` and `Readonly<T>` are shallow. Freeze deeply only at boundaries
  where mutation bugs justify it, and test runtime freezing if it is a contract.
- Do not introduce event sourcing merely to get an audit log. Persisting a
  small audit event stream beside ordinary state is often enough.

**Checks.** Model/property-test arbitrary command sequences, illegal commands,
replay determinism, and event application. Snapshots may supplement, never
replace, explicit invariant assertions.

## Type-level tests are executable specifications

Use the repository's established type-test runner (`expect-type`, `tsd`, or
Vitest's `expectTypeOf`). Type tests complement runtime tests; they do not
execute domain behavior.

```ts
import { expectTypeOf } from 'vitest';

expectTypeOf(parseInvoiceId).returns.toEqualTypeOf<InvoiceId>();

// @ts-expect-error customer and invoice identifiers are not interchangeable
const invalid: InvoiceId = {} as CustomerId;
```

The `as CustomerId` in a deliberately invalid type fixture is not a production
validation pattern. Prefer constructing typed fixtures through their factories
when practical.

- Include positive tests so the API still accepts valid use.
- Put `@ts-expect-error` on the exact invalid expression. The compiler fails if
  the expression becomes valid, preventing stale negative tests.
- Do not snapshot entire compiler output unless the diagnostics are the API.
- Test public declarations from a consumer fixture, not only source-internal
  types. Run `tsc --noEmit` over those fixtures in CI.

## Thinking-OS worked examples

- Frontmatter and inbox events: parse raw YAML/JSON into `unknown`, then a
  schema. Keep transport state separate from a validated domain event.
- Task/project state: use discriminated unions where fields differ by status;
  do not create a union merely to duplicate one shared payload.
- Svelte shared state: reducers and explicit transitions fit complex workflows;
  ordinary form state should remain simple runes state.
- API errors: define a closed application error union before mapping to the UI;
  do not branch on display-message text.

## Review checklist

- Can two semantically different IDs be exchanged accidentally?
- Can optional fields represent a state the domain forbids?
- Is every new union variant handled at compile time?
- Is every cast tied to a nearby runtime proof and boundary?
- Are expected errors exhaustive and unexpected errors contextualized?
- Do type tests prove both valid and invalid uses of the public API?

_Provenance:_ house patterns grounded in the TypeScript Handbook and
typescript-eslint's typed rules. No immutable source has been ingested for this
page yet: <https://www.typescriptlang.org/docs/handbook/>,
<https://typescript-eslint.io/rules/switch-exhaustiveness-check/>,
<https://www.typescriptlang.org/docs/handbook/release-notes/typescript-4-9.html>.

Back to [[ts-architecture]] · [[library]].
