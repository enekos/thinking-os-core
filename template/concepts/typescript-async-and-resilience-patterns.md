---
title: TypeScript async and resilience patterns
summary: "TypeScript patterns for structured cancellation, bounded fan-out, retry budgets, idempotent consumers, outboxes, stale responses, resource disposal, and deterministic failure-injection tests"
type: concept
updated: 2026-11-17
edges:
  - "APPLIES_TO osagai"
---

# TypeScript async and resilience patterns

An async operation needs an owner, deadline, cancellation path, error channel,
and bound. `async`/`await` provides syntax, not lifecycle management. General
TypeScript rules live in [[ts-architecture]]; deeper test selection lives in
[[verification-patterns]].

## The operation contract

Before adding an async abstraction, answer:

- Who starts and who owns completion?
- What ends it: success, error, timeout, caller cancellation, or shutdown?
- What is the maximum concurrency, queue depth, attempts, and elapsed time?
- What happens to work already started when a sibling fails?
- Is repeating the operation safe after an ambiguous timeout?
- Where does the final error go?

If these have no answers, a lint rule can catch a floating promise but not the
resulting lifecycle bug.

## Cancellation propagation

**Use when.** An operation outlives a single `fetch` or composes several steps.

**Invariant.** Every descendant receives the same terminal cancellation signal
and stops allocating new work after cancellation.

```ts
async function loadInvoice(id: InvoiceId, signal: AbortSignal): Promise<Invoice> {
  signal.throwIfAborted();
  const response = await fetch(`/api/invoices/${encodeURIComponent(id)}`, { signal });
  if (!response.ok) throw new HttpError(response.status);
  return invoiceSchema.parse(await response.json());
}
```

- Accept `AbortSignal` as a parameter; do not hide an `AbortController` inside
  every callee. The operation's owner creates the controller.
- Check the signal between expensive non-await steps and before enqueueing more
  work. A signal does not interrupt a synchronous CPU loop by itself.
- Preserve the abort reason where the platform supports it.
- Clean up event listeners and timers in `finally`; a completed operation must
  not retain its parent through a listener.
- Do not catch cancellation, log it, and return a success-shaped empty value.

**Checks.** Abort before start, during I/O, between steps, and after completion.
Assert no subsequent adapter calls, listener/timer leaks, or success callbacks.

## Structured concurrency

**Use when.** Child operations share a parent lifetime and failure policy.

**Invariant.** When the parent finishes, no child remains unobserved.

`Promise.all` rejects early but does not cancel the other promises. Wrap a group
only when every child honors the signal:

```ts
async function runGroup<T>(
  parent: AbortSignal,
  operations: ReadonlyArray<(signal: AbortSignal) => Promise<T>>,
): Promise<T[]> {
  const controller = new AbortController();
  const forwardAbort = () => controller.abort(parent.reason);
  if (parent.aborted) forwardAbort();
  parent.addEventListener('abort', forwardAbort, { once: true });

  const pending = operations.map((operation) => operation(controller.signal));
  try {
    return await Promise.all(pending);
  } catch (error) {
    controller.abort(error);
    await Promise.allSettled(pending);
    throw error;
  } finally {
    parent.removeEventListener('abort', forwardAbort);
  }
}
```

- This helper waits for started children after a failure. A child that ignores
  cancellation can therefore delay completion; that child is broken.
- `Promise.allSettled` observes failures but waits for everything and can hide
  the need for cancellation. Use it when independent completion is intentional.
- `Promise.race` does not cancel losers. A timeout race without aborting the
  underlying operation leaks work.
- For application-scale supervision, use the runtime/library's established
  structured-concurrency primitive instead of growing a custom helper.

**Checks.** Use controllable deferred promises. Resolve/reject children in every
order and assert that all rejections are observed, listeners are removed, and no
child survives group completion.

## Bounded fan-out

**Use when.** A collection controls how much I/O or work is started.

```ts
async function mapBounded<T, R>(
  items: readonly T[],
  limit: number,
  signal: AbortSignal,
  operation: (item: T, index: number, signal: AbortSignal) => Promise<R>,
): Promise<R[]> {
  if (!Number.isInteger(limit) || limit < 1) throw new RangeError('invalid concurrency limit');
  const iterator = items.entries();
  const results: R[] = [];

  async function worker(): Promise<void> {
    for (const [index, item] of iterator) {
      signal.throwIfAborted();
      results[index] = await operation(item, index, signal);
    }
  }

  const count = Math.min(limit, items.length);
  await Promise.all(Array.from({ length: count }, () => worker()));
  return results;
}
```

The shared iterator is advanced synchronously before each `await`, so workers do
not select the same entry. Failure/cancellation still needs the structured
group policy above if siblings must stop and be drained.

- Bound by concurrency and, where relevant, item bytes or cost. Ten huge jobs
  may still exceed the memory budget.
- Do not allocate a promise for every item when the input itself is unbounded.
  Use a stream/iterator and start only up to the limit.
- Preserve input order only when callers need it; ordered result arrays can
  retain memory while slow items finish.

**Checks.** Track current and peak in-flight operations. Assert the peak never
exceeds the limit, every index appears once, empty input works, and failure does
not leave unobserved promises.

## Timeout composition and retry budget

**Use when.** Calling an unreliable dependency.

**Invariant.** One logical operation has one overall deadline and a classified,
bounded retry policy.

| Decision | Rule |
|---|---|
| Retryable error | only classified transient/network/rate-limit/unavailable cases |
| Attempts | fixed small maximum; no recursive unbounded retry |
| Deadline | overall deadline includes waits and every attempt |
| Delay | exponential backoff with jitter; honor valid server retry hints |
| Safety | idempotent operation, idempotency key, or explicit reconciliation |
| Cancellation | caller cancellation interrupts both the attempt and the sleep |
| Observability | report attempts and exhausted budget without logging secrets |

A timeout that merely rejects the caller while leaving the request active is
not resource control. Compose an operation-local controller with the caller's
signal, and distinguish deadline expiration from caller cancellation.

**Checks.** Fake timers plus a controllable transport. Verify no retry on
validation/authorization failures, exact attempt count, cancellation during
backoff, total deadline, and behavior when the response commits but the ack is
lost.

## Idempotent consumer

**Use when.** A queue, webhook, or request can be delivered more than once.

**Invariant.** Repeating the same logical operation has the same durable effect.

- Give the operation a stable idempotency/delivery key supplied by the producer
  or derived from a canonical event identity, not a new random value per retry.
- Atomically claim the key and apply the effect in one transaction where
  possible. A separate `exists` check followed by insert is racy.
- Store enough result state to return the original outcome to a duplicate.
- Define retention for dedupe records and behavior for the same key with a
  different payload; that is normally a conflict, not a silent success.
- If the side effect is external, use the provider's idempotency support or a
  durable outbox/reconciliation protocol.

**Checks.** Deliver duplicates sequentially and concurrently; crash after the
effect but before recording completion; replay after restart; reuse the key with
a different payload. Assert one durable effect, not one handler invocation.

## Transactional outbox

**Use when.** State changes and messages must survive process failure without a
distributed transaction.

**Invariant.** The committed state and its pending event are durable together.

1. In one database transaction, update state and insert the outbox event.
2. After commit, a relay claims due events with bounded concurrency.
3. Publish with the event's stable ID and retry transient publish failures.
4. Mark published only after acknowledgement.
5. Consumers remain idempotent because crash between publish and mark creates a
   duplicate.

Do not mark before publishing, and do not hold the state transaction open while
waiting for a broker. Add lease recovery for a relay that crashes after claiming.
Bound event size, attempts, and retention; expose stuck-event observability.

**Checks.** Inject crashes before commit, after commit, after claim, after
publish, and after acknowledgement. Assert no lost committed event and an
acceptable, explicitly tested duplicate window.

## Stale response and UI operation policy

**Use when.** A user can start a newer read/write before the previous finishes.

- Use osagai `query()`/`task()` for the default stack; they own stale-response,
  busy-state, and error plumbing. Do not rebuild those primitives per route.
- Outside the kit, associate each request with an operation ID or abort the
  superseded request. Ignore stale successes and stale errors.
- Distinguish read cancellation from a submitted mutation. Navigating away does
  not prove the server did not commit the mutation.
- Disable duplicate submission only as UX; server idempotency still owns the
  correctness guarantee.

**Checks.** Deferred promises resolve in reverse order. Assert only the newest
read updates state, stale errors do not replace fresh data, and a mutation gets
one stable submission identity.

## Resource disposal

**Use when.** A resource needs deterministic synchronous or asynchronous
cleanup.

- Prefer a scoped function that acquires, `try/finally` releases, and returns no
  escaped resource.
- Where the toolchain and target support explicit resource management, `using`
  and `await using` make scope-bound disposal visible. Verify emitted output and
  library support before adopting them.
- An `asyncDispose`/`Symbol.asyncDispose` implementation must be idempotent and
  surface cleanup failure according to the application's error policy.
- Do not place an exclusive application lock in a disposable object and then
  hold it across unrelated I/O.

**Checks.** Release on normal return, thrown error, and cancellation. Test
double disposal and cleanup failure. Detect handles/listeners left after the
test rather than sleeping until they disappear.

## Thinking-OS worked examples

- `zaindu` polling semantics translated to a TypeScript watcher: overlapping
  polls should be prevented or coalesced, and each poll needs an overall budget.
- Inbox proposal generation: writing state then emitting a proposal is an
  outbox-like durability problem; test crash points before adding concurrency.
- osagai query behavior: reverse-order response tests belong at the primitive's
  conformance suite so every consumer inherits the same stale-response policy.
- Frontmatter import: parse/validate before enqueueing; cancellation after a
  durable write needs an idempotent resume key.

## Review checklist

- Who observes every promise's failure?
- Does every race/timeout stop or deliberately orphan its loser?
- What bounds fan-out, queued work, retries, and elapsed time?
- Can a duplicate delivery commit twice?
- Does cancellation remove listeners/timers and leave a recoverable state?
- Do tests control resolution order and crash points instead of using real sleeps?

_Provenance:_ house patterns grounded in MDN's AbortSignal/Promise semantics,
TypeScript's explicit resource-management documentation, and agent-review
failures. No immutable source has been ingested for this page yet:
<https://developer.mozilla.org/en-US/docs/Web/API/AbortSignal>,
<https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Promise>,
<https://www.typescriptlang.org/docs/handbook/release-notes/typescript-5-2.html>.

Back to [[ts-architecture]] · [[verification-patterns]] · [[library]].
