---
title: Rust resource and concurrency patterns
summary: "Rust patterns for deterministic cleanup, structured task lifecycles, bounded actors, backpressure, cancellation, lock discipline, atomic protocols, and adversarial concurrency verification"
type: concept
updated: 2026-11-17
edges:
  - "APPLIES_TO sutegi"
---

# Rust resource and concurrency patterns

The default is one owner, bounded work, explicit shutdown, and a test that can
force the dangerous interleaving. Read [[rust-style-guide]] for the general
thread-safety and async rules; this page supplies design shapes and verification.

## Choose the ownership shape first

| Pressure | Pattern | Main hazard |
|---|---|---|
| One logical owner, many readers | pass `&T`; shorten borrows | premature `Arc` |
| Shared immutable data | `Arc<T>` | cycles and hidden ownership |
| Independent units of work | move data; use scoped threads | detached work and ignored panics |
| One owner, many command senders | actor over a bounded channel | queue growth and shutdown deadlock |
| Frequent stable references inside one graph | arena/slot map + typed indices | stale or reused indices |
| Shared mutable state is unavoidable | one lock with documented invariants | guard across `.await`, callbacks, or I/O |
| One flag/counter with a proven protocol | atomics | publication and ordering errors |

`Arc<Mutex<T>>` is not an architecture. It may be the right leaf mechanism after
the ownership and lifecycle decisions, not before them.

## RAII guard

**Use when.** Entering a scope creates an obligation: restore state, remove a
file, roll back a transaction, release a permit, or re-enable a service.

**Invariant.** Every exit path performs the synchronous cleanup exactly once.

```rust
struct MaintenanceGuard<'a> {
    state: &'a mut MaintenanceState,
}

impl Drop for MaintenanceGuard<'_> {
    fn drop(&mut self) {
        self.state.in_maintenance = false;
    }
}

impl MaintenanceState {
    fn begin(&mut self) -> MaintenanceGuard<'_> {
        assert!(!self.in_maintenance, "nested maintenance is unsupported");
        self.in_maintenance = true;
        MaintenanceGuard { state: self }
    }
}
```

- Mark guards `#[must_use]`; assigning to `_` drops immediately.
- Prefer borrowing guards when the protected resource must remain unavailable.
- State the panic behavior. `Drop` runs during unwinding, not when the process
  aborts or is killed.
- `Drop` cannot await. Async cleanup needs an explicit async lifecycle or a
  supervisor; dropping a future only cancels it.
- Avoid leaking via `mem::forget`, reference cycles, or indefinitely parked
  scopes unless leakage is an intentional part of the API contract.

**Checks.** Test normal return, early `?`, and `catch_unwind` where unwinding is
supported. Count cleanup calls; do not infer cleanup from timing.

## Structured thread lifecycle

**Use when.** Work may borrow local data or must finish before the parent
returns.

```rust
let mut totals = Vec::new();
std::thread::scope(|scope| {
    let handles: Vec<_> = chunks
        .iter()
        .map(|chunk| scope.spawn(move || checksum(chunk)))
        .collect();
    for handle in handles {
        totals.push(handle.join().expect("checksum worker panicked"));
    }
});
```

- `thread::scope` bounds borrows and joins automatically; explicit `join`
  surfaces the worker's panic and result.
- Long-lived services need a shutdown signal and a join phase. Store every
  `JoinHandle` in the component that owns the service.
- Document whether dropping a handle detaches, cancels, or blocks. Do not let
  each API choose a different silent policy.
- For async tasks, select an explicit supervisor/join-set pattern. Spawning and
  forgetting a task does not transfer its errors to anyone.

**Checks.** Force a child panic, parent early return, shutdown while work is
queued, and a blocked worker. Assert all handles are joined and no later output
arrives after shutdown reports completion.

## Bounded actor and command protocol

**Use when.** One owner should serialize access to state while other components
submit commands.

```rust
enum StoreCommand {
    Put(Page, Sender<Result<(), StoreError>>),
    Snapshot(Sender<Snapshot>),
    Shutdown(Sender<()>),
}
```

- Give the actor exclusive ownership of its store; do not also expose `Arc`
  aliases that bypass message ordering.
- Use `sync_channel(bound)` for threads or bounded `tokio::mpsc::channel` for
  async. Define what senders do at capacity: wait, reject, or shed explicitly.
- Include shutdown in the protocol. Closing all senders may be impossible when
  one is retained by a supervisor.
- A request/response command carries a bounded reply path. Specify timeout and
  actor-drop behavior.
- Never synchronously block an async actor waiting for a thread that needs that
  same runtime to make progress.

**Checks.** Fill the queue, drop the actor before replying, race shutdown with a
request, and inject a store error. Assert bounded memory behavior and that no
reply waiter is left without a defined outcome.

## Backpressure and retry budget

**Use when.** External input or fan-out produces work faster than it can be
completed.

**Invariant.** Every queue, buffer, retry loop, and outstanding-operation set
has a bound and an overload policy.

| Boundary | Default policy |
|---|---|
| Request intake | reject or wait with a deadline before allocating unbounded state |
| Internal channel | bounded queue; propagate capacity to the producer |
| Batch fan-out | fixed worker count; preserve result order only if required |
| Retry | maximum attempts plus an overall deadline and jitter |
| Cache | entry/byte bound and eviction policy |
| Response stream | bounded chunks; stop when the consumer cancels |

Retry only failures classified as transient, and only operations that are
idempotent or carry an idempotency key. Per-attempt timeouts without an overall
deadline multiply latency. Retrying a full pipeline retries every inner retry
unless the budget is explicitly shared.

**Checks.** Use fake clocks and a fake transport. Verify exact attempts,
deadline, cancellation, jitter range, and that a permanent error is not retried.
Measure queue depth under overload rather than merely asserting eventual success.

## Lock discipline

**Use when.** Shared state cannot be replaced by ownership transfer or messages.

**Invariant.** One lock protects one documented consistency boundary.

- Keep the critical section to field updates. Drop the guard before `.await`,
  file/network I/O, user callbacks, or acquiring another lock.
- Perform check-and-act under one acquisition. Unlocking to recompute and then
  relocking creates a time-of-check/time-of-use race.
- If multiple locks are unavoidable, document a global acquisition order and
  test the reverse-order contender.
- Choose and document poisoning behavior: propagate, recover with proof, or
  abort. `.lock().unwrap()` is not a policy explanation.
- `std::sync::MutexGuard` is not an async lock. An async mutex still does not
  make holding a guard across I/O a good design.

**Checks.** Clippy must deny `await_holding_lock` and
`await_holding_refcell_ref`. Deterministically pause one contender between two
steps using barriers/channels; `sleep` is not an interleaving test.

## Atomic protocol

**Use when.** Measurements show a lock is the bottleneck and the protocol is
small enough to specify formally.

Write the protocol before the code:

1. State protected by the protocol.
2. States/transitions represented by each atomic value.
3. Which store publishes which data.
4. Which load observes that publication.
5. Why each `Ordering` is necessary.
6. ABA, wraparound, overflow, and shutdown behavior.

`SeqCst` everywhere is not proof of correctness; `Relaxed` does not publish
other data by itself. A flag guarding another allocation needs a proven
release/acquire relationship or a stronger ordering.

**Checks.** Every hand-written protocol gets a reduced `loom` model with small
counts and explicit interleaving exploration. Run Miri for unsafe cells and use
ThreadSanitizer on supported targets where appropriate. Keep the full model's
state space bounded so CI remains useful.

**Avoid when.** A mutex or channel satisfies the measured requirement.

## Cancellation-safe transaction

**Use when.** Dropping a future or returning early can leave multi-step work
partially applied.

- Make each commit atomic at the persistence boundary. Do not hold a database
  transaction open across unrelated network calls.
- On cancellation, either roll back before returning or leave enough durable
  state for idempotent recovery.
- If cleanup must complete, move the operation to an owned supervised task;
  relying on the caller to poll a future to completion is not durability.
- For file replacement, write and sync a temporary file, then atomically rename
  according to the filesystem's guarantees. Test crash points separately from
  ordinary errors.

**Checks.** Inject failure or cancellation after every durable step. Restart the
component and assert either the whole operation is absent or its completion can
be safely resumed—never an undocumented half-state.

## Thinking-OS worked examples

- `tools/src/watch/events.rs`: event application should be deterministic and
  idempotent under duplicate poll delivery; property-test repeated application.
- `tools/src/watch/gh.rs`: external command and GitHub polling need explicit
  timeouts and bounded result sizes. Test timeout, malformed output, and poll
  shutdown independently.
- `tools/src/kg/model.rs`: atomic file replacement is an RAII/transaction case;
  test write errors and stale temporary files, not only successful writes.
- `zaindu`: a watcher can own its ledger while polling and proposal production
  remain separate stages. Introduce channels only when measured overlap or an
  independent lifecycle justifies them.

## Review checklist

- Who owns each resource, task, handle, and shutdown signal?
- What bounds every queue and what happens at capacity?
- Can a guard survive an `.await`, callback, panic, or second lock?
- Does cancellation leave a recoverable state at every await/durable boundary?
- Is each atomic ordering justified by a publication protocol?
- Does the test force the failure interleaving instead of hoping a sleep works?

_Provenance:_ house patterns grounded in the Rust Book, Rust Atomics and Locks,
Tokio's cancellation and channel documentation, and agent-review failures. No
immutable source has been ingested for this page yet:
<https://doc.rust-lang.org/book/>,
<https://marabos.nl/atomics/>,
<https://tokio.rs/tokio/topics/shutdown>,
<https://docs.rs/loom>.

Back to [[rust-style-guide]] · [[verification-patterns]] · [[library]].
