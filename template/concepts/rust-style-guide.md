---
title: Rust style guide & agent gotchas
summary: "Rust rules for agents — ownership, thread safety, unsafe, security, perf, lint baseline; read before any Rust edit"
type: concept
updated: 2026-11-17
sources:
  - sources/2026-06-10-ms-rust-interop.md
edges:
  - "EVIDENCED_BY 2026-06-10-ms-rust-interop"
  - "APPLIES_TO sutegi"
---

# Rust style guide & agent gotchas

Opinionated defaults for Rust across your projects, written against the failure modes LLM agents produce. A crate's own `clippy.toml` / `rustfmt.toml` / `[lints]` win on conflict. Toolchain: latest stable, **edition 2024** for new crates (MSRV ≥ 1.85).

Order of priorities when they collide: **sound → secure → correct → maintainable → fast**. Never trade a level for the one below it without a measurement and a written reason.

**Contents:** [Checklist](#before-you-say-done--the-agent-checklist) · [Architecture](#architecture-defaults) · [Memory](#memory-management) · [Threads](#thread-safety) · [Async](#async--the-agent-landmine-field) · [`unsafe`](#unsafe-policy) · [Security](#security) · [Performance](#performance) · [Errors](#errors) · [Types & API](#types--api) · [Testing](#testing-for-memory-and-thread-bugs) · [Lint baseline](#lint-baseline--paste-into-every-workspace) · [Agent mistakes](#things-agents-reliably-get-wrong) · [Reference shelf](#reference-shelf--where-these-rules-come-from) · [Pattern fit](#pattern-fit)

## Before you say "done" — the agent checklist

Run these. Don't describe them, run them:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo +nightly miri test -p <crate>     # if the diff touches unsafe
cargo deny check                        # if the diff touches Cargo.toml
```

Then, for the diff only:

1. Every new `.clone()`, `Arc`, `Mutex` and `unsafe` has a reason you can say in one sentence.
2. No `unwrap()` / `expect()` / `[i]` / `as` cast on a path that **untrusted input** reaches.
3. No lock guard lives across `.await`, I/O, a callback, or a second lock.
4. Every spawned thread/task has an owner that joins it or cancels it on shutdown.
5. Every queue, cache, buffer and read from the network has an **upper bound**.
6. Public API changed only on purpose: `pub` items are a commitment.

## Architecture defaults

- **One workspace per project.** Root `[workspace]`; shared deps in `[workspace.dependencies]`, members use `dep = { workspace = true }`. Lints in `[workspace.lints]`, members opt in with `[lints] workspace = true`.
- **Crate split = stable seam, not file count.** Split when the dependency footprint differs, it's published, or two binaries reuse it.
- **`lib.rs` is the API.** Re-export the surface explicitly (`pub use foo::Bar;`); keep modules `pub(crate)`.
- **`foo.rs` + `foo/`, not `foo/mod.rs`.**
- **Visibility ladder.** Default `pub(crate)`. Bare `pub` only at the published surface.
- **Binary crates stay thin.** `main.rs` parses args and wires; logic lives in the lib so it's testable.
- **Features are additive.** Enabling a feature never breaks another consumer. No mutually exclusive features.
- **Cargo.lock is committed**, libraries included.
- **Make invalid states unrepresentable.** An enum with data per state beats a struct of `Option`s and a `status` field. Typestate (`Conn<Open>` / `Conn<Closed>`) only where misuse is costly and the API is small.

## Memory management

Rust frees memory for you; it does not stop you from holding too much of it, holding it too long, or building a structure that can't be freed. Those three are the real risks.

### Ownership design — decide who owns what before writing types

- **Data forms a tree of owners.** Each value has one owner; everything else borrows or holds an index/ID. When you feel the need for a parent pointer, a back-reference, or a graph, stop: that's a design decision, not a borrow-checker fight.
- **Graphs and cyclic structures → arena + indices.** `Vec<Node>` with `NodeId(u32)` handles (or `slotmap` when you need removal with stale-handle detection). Never `Rc<RefCell<Node>>` with child pointers both ways — it leaks on cycles and panics at runtime on re-borrow.
- **Back-references when you must → `Weak`.** Parent owns child with `Rc`/`Arc`; child points up with `Weak`. A cycle of strong `Rc`/`Arc` is a memory leak that no tool will report.
- **Borrow on the way in, own on the way out.** Params `&str`, `&[T]`, `&Path`, `impl AsRef<str>`; return owned values unless borrowing from `&self`.
- **Take ownership when you'll store it.** `fn new(name: String)` beats `fn new(name: &str)` followed by `.to_string()` inside — the caller may already own one and can move it in for free.

### Smart pointer decision table

| Need | Use | Not |
|---|---|---|
| Heap allocation, single owner, recursive type, fat enum variant | `Box<T>` | `Rc` "just in case" |
| Shared read-only, one thread | `Rc<T>` | `Arc` (atomic cost for nothing) |
| Shared read-only, many threads | `Arc<T>` | `Arc<Mutex<T>>` when nobody writes |
| Shared immutable string/slice | `Arc<str>`, `Arc<[T]>`, `Box<str>` | `Arc<String>` (double indirection) |
| Sometimes borrowed, sometimes owned | `Cow<'a, T>` | always cloning |
| Shared + mutated, many threads | ownership transfer / channel first; then `Arc<Mutex<T>>` | `Rc<RefCell<T>>` across threads (won't compile — good) |
| Global initialised once | `std::sync::LazyLock` / `OnceLock` | `lazy_static!`, `once_cell`, `static mut` |
| Interior mutability, one thread, `Copy` data | `Cell<T>` | `RefCell` for a counter |

### RAII for every resource, not only memory

- **Every resource is a value whose `Drop` releases it**: files, sockets, fds, locks, temp dirs, child processes, spans, transactions. If cleanup needs to be remembered by the caller, the API is wrong.
- **OS handles → `OwnedFd` / `BorrowedFd` / `AsFd`**, never a bare `RawFd` with a hand-written `close`. `OwnedFd` closes exactly once and can't be used after close; a `RawFd` field can be copied, double-closed, or closed while another thread polls it. *Live instance:* a hand-rolled epoll/kqueue poller stored `fd: RawFd` and closed it in `Drop` — the fix is `OwnedFd`.
- **Child processes**: keep the `Child` and `wait()` it, or it's a zombie. `kill_on_drop(true)` on `tokio::process::Command`.
- **Drop order is reverse declaration order** for locals and declaration order for struct fields. If correctness depends on it (a guard must drop before the thing it guards), make it explicit with a scope `{ }` or `drop(x)` — don't rely on line order a refactor will shuffle.
- **Never `mem::forget`** a value that owns a resource. `Box::leak` only for true process-lifetime data (parsed config), once, at startup.
- **`Drop` must not panic and must not block indefinitely.** Fallible cleanup (flush, commit) gets an explicit `fn close(self) -> Result<()>`; `Drop` is the best-effort fallback.

### Bound everything that grows

The leak Rust can't stop is the one you asked for.

- **Unbounded channels are unbounded memory.** Default to `mpsc::channel(n)` / `sync_channel(n)`. `unbounded_channel` needs a written reason why the producer can't outrun the consumer.
- **Caches have a capacity and an eviction policy.** A `HashMap` that's only ever inserted into is a leak with extra steps.
- **Reads from the network or disk have a max size.** `take(MAX)` on readers, length checks before `Vec::with_capacity(len_from_header)` — an attacker-chosen capacity is an OOM.
- **Long-lived `Vec`/`String` buffers that spike**: `shrink_to(n)` after the spike, or the high-water mark stays allocated forever.
- **Per-connection/per-request state is removed on disconnect/error paths too**, not only on the happy path.

### Allocation discipline

- **Pre-size** when length is known: `Vec::with_capacity`, `String::with_capacity`, `HashMap::with_capacity`.
- **Reuse buffers in loops**: allocate once outside, `.clear()` inside. `String::new()` / `format!` / `Vec::new()` inside a hot loop is N allocations.
- **`write!(&mut buf, …)`** into an existing `String` over `format!` + `push_str`.
- **`mem::take` / `Option::take` / `mem::replace`** to move out of `&mut` instead of cloning.
- **`.copied()` / `.cloned()` / `.to_vec()`** over hand-written map-clone-collect.
- **Enum size = largest variant.** `Box` the fat variant (`clippy::large_enum_variant` warns). Pin sizes of hot types: `const _: () = assert!(std::mem::size_of::<Token>() <= 16);`.
- **Don't `collect()` just to iterate again.** Chain the iterator.
- **`SmallVec` / `ArrayVec` / arenas (`bumpalo`) only after a profile** says allocation is the cost.

## Thread safety

### The model in one paragraph

`Send`: safe to *move* to another thread. `Sync`: safe to *share* `&T` between threads (`T: Sync` ⇔ `&T: Send`). The compiler checks both; you can only break them with `unsafe impl`. So in safe Rust, data races are impossible — **what remains is deadlocks, races on logic (check-then-act), starvation, lost wake-ups, and unbounded queues**. Those are what to review for.

### Choose the primitive — in this order

1. **No sharing.** Move the data into the thread/task; get the result back from the `JoinHandle`. Most "shared state" is really a hand-off.
2. **`std::thread::scope`** when worker threads need to *borrow* stack data. No `Arc`, no `'static`, automatic join. Prefer it over `thread::spawn` + `Arc::clone` for fork-join work.
3. **Channels** for pipelines and ownership transfer: `std::sync::mpsc` / `crossbeam-channel` (sync), `tokio::sync::{mpsc, oneshot, watch, broadcast}` (async). Bounded by default.
4. **Actor**: one task/thread owns the state; others send it messages. The cleanest answer when many parties mutate one thing.
5. **`Mutex<T>`** for small shared state with short critical sections. **`RwLock<T>`** only when reads dominate *and* critical sections are long enough to matter — it's slower than `Mutex` under write contention and can starve writers.
6. **Atomics** for counters, flags, and IDs. Never to build your own lock or lock-free structure — use `crossbeam` / `arc-swap` / `dashmap`, which have been verified.

### Lock discipline

- **The lock protects data, not code.** Put the data *inside* the `Mutex<T>`; never `Mutex<()>` next to the fields it "guards".
- **Shortest possible critical section.** Lock, copy/clone/swap out what you need, unlock, *then* do the work. `let snapshot = state.lock()?.clone();` is often right.
- **Never, while holding a guard**: `.await`, block on I/O, call a user callback/closure/trait object, log through something that might lock, send on a bounded channel, or take a second lock. Each of these is a deadlock or a latency cliff waiting for load.
- **Two locks → a documented order**, enforced by a type that only exposes the correct sequence, or merge them into one lock over a struct.
- **Check-then-act happens under one lock acquisition.** `if map.lock().contains(k) { map.lock().insert(..) }` is a race; use the `entry` API under a single guard.
- **No `Rc`, `RefCell`, or raw pointers inside a `Mutex`** to "make it Send" — that's `unsafe impl Send` with extra steps.
- **`Arc<Mutex<Vec<T>>>` filled by N workers** is usually a channel or `thread::scope` returning results.

### Poisoning — pick a policy, don't just `.unwrap()`

A `std::sync::Mutex` is *poisoned* when a thread panics while holding it. There are ~325 `.lock().unwrap()` call sites across the repos. Propagating is the sanctioned default (MS **M-PANIC-IS-STOP**: a poison means another thread already panicked, so stopping is correct) — but it should be a choice, not a reflex.

- **Invariant could be half-updated** (the default) → propagate: `.lock().expect("state mutex poisoned")`.
- **State is valid regardless** (counters, caches, a `Vec` of log lines) → recover: `.lock().unwrap_or_else(std::sync::PoisonError::into_inner)`.
- **Wrap it once** in a helper (`fn lock(&self) -> MutexGuard<'_, T>`) so the policy lives in one place, not 40.
- `parking_lot` and `tokio::sync::Mutex` don't poison — consciously accept that a panicked writer may have left state half-written.

### Atomics — memory ordering

- **`Relaxed`** is correct for standalone counters, ID generators, and a stop flag that publishes nothing else. Most uses across these repos are exactly that — fine.
- **`Release` store / `Acquire` load** the moment a flag or pointer *publishes other data* ("ready = true means the buffer is filled"). `Relaxed` there is a real bug that only reproduces on ARM — i.e. on this Mac.
- **`SeqCst` is not a "safe default"** — it's a sign nobody thought about it. Use it only when you need a single total order across multiple atomics, and say so in a comment.
- **`AtomicBool` + `Mutex` together is a smell**: put the bool inside the mutex.
- Prefer `fetch_update` / `compare_exchange` loops over load-then-store.
- Any hand-written atomic protocol gets a `loom` test.

### Globals

- **`static mut` is banned.** Edition 2024 denies references to it. Use `LazyLock<Mutex<T>>`, `OnceLock<T>`, or an atomic.
- **Prefer no globals** (MS **M-AVOID-STATICS** — a static is duplicated per crate version, so two copies of the "one" counter can coexist). Statics for caches are fine; statics for correctness are not. Pass a context/handle; globals make tests share state (the classic trap: parallel tests sharing one global DB handle).
- **Migrate** remaining `lazy_static!` / `once_cell` to `std::sync::LazyLock` / `OnceLock` when touching the file.

### Thread and task lifecycle

- **Every spawn has an owner.** Keep the `JoinHandle` and `join()` it; a dropped `JoinHandle` silently detaches and swallows the panic. Use `JoinSet` / `TaskTracker` for groups of tokio tasks.
- **Propagate worker panics**: `handle.join()` returns `Err` on panic — surface it, don't `let _ =` it.
- **Shutdown is designed, not hoped for**: a cancellation token / `watch` channel / closed sender tells workers to stop; `main` waits for them with a deadline.
- **Name threads** (`thread::Builder::new().name(..)`) — panics and profilers become readable.
- **Blocking work never runs on an async worker**: `spawn_blocking` for file I/O, CPU-heavy work, sync DB drivers (`rusqlite`). A blocked worker stalls every task scheduled on it.

## Async — the agent landmine field

- **One runtime per workspace** (Tokio). `cargo tree -e features` if unsure.
- **`std::sync::Mutex` is fine in async code** — Tokio's own docs recommend it — *if* the guard never crosses `.await`. A guard across `.await` makes the future `!Send` (won't spawn) and, on a current-thread runtime, deadlocks when another task on that thread locks. Use `tokio::sync::Mutex` only when you truly must hold across `.await` (e.g. serialising access to an async connection).
- **`async fn` in traits** is stable but not dyn-compatible and carries no `Send` bound. For `dyn` use `async_trait` or a manual `Pin<Box<dyn Future + Send>>`; for generic `Send` futures, `trait-variant` or `-> impl Future<Output = T> + Send`.
- **`Rc`/`RefCell` are `!Send`** — they poison any future that holds them across `.await`.
- **Anticipate `Send + 'static`** in generic async bounds from the start.
- **Cancellation safety.** `select!` drops the losing future mid-flight. Anything that's buffered a partial read/write or half-updated state is not cancel-safe — document which futures are, and restructure (move the state out of the future) if one isn't.
- **Timeouts on every external call** (`tokio::time::timeout`). An await without a deadline is an unbounded wait.
- **Never `block_on` inside async; never `tokio::spawn` from sync code** without a runtime handle — it panics at runtime.
- **`clippy::large_futures`**: deep async state machines blow the stack; `Box::pin` the big one (MS **M-ASYNC-STACK-SIZE**).
- **CPU loops inside async yield** (`tokio::task::yield_now().await` every N items) or move to `spawn_blocking` — MS **M-YIELD-POINTS**.

## `unsafe` policy

- **Three valid reasons only** (MS **M-UNSAFE**): a novel abstraction, a measured performance need, or FFI/syscalls. Never to shorten safe code, bypass `Send`, or stretch a lifetime with `transmute`.
- **Soundness is absolute** (MS **M-UNSOUND**): a safe fn that can cause UB under *any* calling pattern is a bug, however contrived the pattern.
- **Default `unsafe_code = "deny"`** at workspace level. The one module that needs it opts in with `#[allow(unsafe_code)]` at module scope, so the blast radius is visible in one grep.
- **Prefer safe wrappers that exist**: `rustix` / `nix` over raw `libc`, `OwnedFd` over `RawFd`, `bytemuck` / `zerocopy` over `transmute`, `MaybeUninit` over `mem::zeroed()` for non-POD types.
- **Encapsulate.** `unsafe` lives in a small module that exposes a *safe* API whose invariants the module itself upholds. Callers should never need `unsafe` to use it correctly.
- **`// SAFETY:` on every block** — the one exception to a no-comments house style, because `clippy::undocumented_unsafe_blocks` requires it — stating which invariant makes it sound; **`# Safety`** rustdoc on every `unsafe fn` stating what the caller must guarantee. *Live instance:* a socket poller with ~15 `unsafe` blocks and zero `SAFETY` comments, in a workspace with no lint table to catch it.
- **One unsafe operation per block** (`clippy::multiple_unsafe_ops_per_block`) so each gets its own justification.
- **`unsafe impl Send/Sync`** needs a paragraph, not a line, and a reviewer.
- **Miri** (`cargo +nightly miri test`) on any crate with `unsafe` that isn't pure FFI. ThreadSanitizer (`RUSTFLAGS=-Zsanitizer=thread`, nightly) for custom sync code.

### M-ESCAPE-HATCHES — native handles (Microsoft guideline, enforced)

A type wrapping an FFI/native handle provides `pub unsafe fn from_native(..) -> Self` (safety invariants documented), `pub fn into_native(self)`, and `pub fn to_native(&self)`.

## Security

- **Untrusted input never panics.** On any path a request, file, or CLI arg reaches: no `unwrap`/`expect`, no `slice[i]` (use `.get(i)`), no `&s[a..b]` on `str` (char-boundary panic), no `as` narrowing casts (`u32::try_from(x)?`). A panic in a server is a DoS.
- **Integer arithmetic on sizes/offsets from input** uses `checked_*` / `saturating_*`. Release builds wrap silently on overflow; set `overflow-checks = true` in `[profile.release]` for parsers, crypto, and network servers.
- **Bound recursion and size**: max body length, max nesting depth for JSON/YAML/AST parsing, max header count. `serde_json` has a recursion limit; hand-written parsers don't unless you add one.
- **Shell out without a shell.** `Command::new("git").args([..])`, never `Command::new("sh").arg("-c").arg(format!(..))`. Pass user data as separate args, after `--`.
- **SQL is parameterised**, always (`?`/`$1` bindings); never `format!` into a query.
- **Paths from input**: reject `..` components and absolute paths, or `canonicalize()` and check `starts_with(root)`. Beware TOCTOU between the check and the open.
- **Secrets**: wrap in a newtype with a redacting `Debug`/`Display` (or `secrecy::SecretString`) so they can't reach logs; `zeroize` key material on drop; compare tokens/MACs with `subtle::ConstantTimeEq`, never `==`.
- **Crypto**: never hand-rolled. `rustls` over `openssl`; `ring` / RustCrypto crates; randomness from `getrandom` / `rand::rngs::OsRng` for anything security-relevant.
- **HashDoS**: `std` `HashMap` (SipHash) for attacker-controlled keys. `foldhash` / `FxHash` for trusted internal keys only (MS **M-FAST-HASHER** — the speed win is real, the trust condition is the whole rule).
- **Config/request deserialisation**: `#[serde(deny_unknown_fields)]` on config; explicit size limits on request bodies before deserialising.
- **Supply chain**: every new dependency is a decision. Check maintenance and `unsafe` usage (`cargo geiger`), use `default-features = false`, run `cargo deny check advisories licenses bans` in CI, `cargo machete` to prune.

## Performance

- **Measure first.** `criterion` or `divan` for micro-benchmarks, `samply` / `cargo flamegraph` for profiles, `cargo bloat` for binary size, `hyperfine` for CLIs. An optimisation without a before/after number gets reverted.
- **`panic = "abort"` only where nothing relies on `catch_unwind`.** It turns every recovered panic (a server answering 500, an actor supervisor restarting a child) into a process exit, and only in release, so debug-built tests never see it. *Live instance:* a web framework's workspace profile set `abort` under an HTTP server and actor supervisor that both `catch_unwind`.
- **Release profile:** `lto = "thin"` (or `true`), `codegen-units = 1`, `strip = true`. Add `debug = "line-tables-only"` when you need profiles.
- **Buffered I/O.** `BufReader` / `BufWriter` around files and sockets; lock `stdout` once (`let mut out = io::stdout().lock();`) in output loops. Agents forget this more than anything else in this section.
- **Clones of large data in hot paths** → borrow, `Arc`, or restructure. `clippy::clone_on_ref_ptr` makes `Arc::clone(&x)` explicit so cheap and expensive clones look different.
- **Static over dynamic dispatch in hot loops** (`impl Trait`, generics); `dyn` where fan-out would blow compile time. `Vec<MyEnum>` over `Vec<Box<dyn Trait>>` when the set of variants is closed.
- **Contention is the perf killer in concurrent code.** Shard locks, batch updates, or move to per-thread state merged at the end — before reaching for lock-free.
- **`#[inline]`** only on small cross-crate functions, after measuring.

## Errors

- **Panics are for bugs, `Result` is for conditions** (MS **M-PANIC-IS-STOP**, **M-PANIC-ON-BUG**). A broken invariant panics; bad input returns `Err`. Never use a panic to signal an error upstream, and never assume it'll be caught — a consumer may build with `panic = "abort"`.
- **Libraries: `thiserror`**, one enum per crate, `#[from]` to lift sub-errors. (MS **M-ERRORS-CANONICAL-STRUCTS** prefers per-situation structs carrying a `Backtrace`; house style stays an enum, but reach for a struct when callers need accessor methods.)
- **Binaries: `anyhow`**, `.context("doing X")` at boundaries.
- **No `unwrap()` outside tests / build scripts / init.** `.expect("why it can't fail")` documents an invariant; better, make the type enforce it.
- **`?` with `From`**, not `.map_err` chains.
- **`# Errors` / `# Panics`** rustdoc on any `pub fn` that does either.
- **Never discard a `Result`** with `let _ =` without a comment saying why it's safe to ignore.

## Types & API

- **Newtype the domain** (`OrgId(Uuid)`, `NodeId(u32)`), with `From` / `AsRef` / `Display`.
- **`impl Trait` params/returns** unless you need `dyn`.
- **Derive `Debug` always** — except types holding secrets, which get a redacting impl. Derive `Clone`/`Default`/`PartialEq`/`Hash` only when used.
- **`#[non_exhaustive]`** on public enums/structs that may grow.
- **`#[must_use]`** on builders, guards, and functions whose result is the point.
- **`From` / `TryFrom` / `FromStr` / `Display`** over bespoke `to_x()` / `parse_x()`.
- **`#[expect(lint, reason = "…")]` over `#[allow]`** — it errors when the lint stops firing, so stale suppressions don't pile up.

### Microsoft interop guidelines (enforced) — sources/2026-06-10-ms-rust-interop.md

- **M-TYPES-SEND** — public futures and most public types are `Send`. Assert it:
  ```rust
  const fn assert_send<T: Send>() {}
  const _: () = assert_send::<MyPublicType>();
  #[test] fn entry_future_is_send() { fn s<T: Send>(_: T) {} s(my_async_entry()); }
  ```
- **M-DONT-LEAK-TYPES** — no third-party types in public APIs unless sibling-crate, feature-gated (`serde`), or a genuine interop win. `cargo public-api` before releases.
- **M-ESCAPE-HATCHES** — see `unsafe` policy above.

## Testing for memory and thread bugs

- **`proptest`** for parsers, encoders and anything with an inverse (round-trip properties). **`cargo fuzz`** for anything parsing untrusted bytes.
- **`loom`** for any custom synchronisation — it explores interleavings that a test run never hits.
- **Miri** for `unsafe`; catches UB, use-after-free, and invalid aliasing.
- **Deterministic time**: `tokio::time::pause()` + `advance()`, never `sleep` to "let it settle".
- **Concurrency tests assert outcomes, not timing.** Use barriers/channels to force an interleaving; a test that passes because a sleep was long enough is flaky forever.
- **Tests don't share globals.** Each test builds its own state/DB; parallel `cargo test` will expose you otherwise.

## Lint baseline — paste into every workspace

A workspace without a lint table gets this the next time you touch it.

```toml
[workspace.lints.rust]
unsafe_code = "deny"               # opt in per module with #[allow(unsafe_code)]
unsafe_op_in_unsafe_fn = "deny"
missing_debug_implementations = "warn"

[workspace.lints.clippy]
# concurrency
await_holding_lock = "deny"
await_holding_refcell_ref = "deny"
rc_mutex = "deny"
mutex_atomic = "warn"
large_futures = "warn"
# unsafe
undocumented_unsafe_blocks = "deny"
missing_safety_doc = "deny"
multiple_unsafe_ops_per_block = "deny"
# memory / resources
mem_forget = "deny"
clone_on_ref_ptr = "warn"
large_enum_variant = "warn"
# correctness / hygiene
unwrap_used = "warn"
let_underscore_must_use = "warn"
dbg_macro = "deny"
todo = "deny"
allow_attributes_without_reason = "warn"
# input-facing crates only (parsers, servers): uncomment
# indexing_slicing = "warn"
# string_slice = "warn"
# arithmetic_side_effects = "warn"
# cast_possible_truncation = "warn"

[profile.release]
lto = "thin"
codegen-units = 1
strip = true
# overflow-checks = true           # parsers, crypto, network servers
```

```toml
# clippy.toml
allow-unwrap-in-tests = true
allow-expect-in-tests = true
disallowed-types = [
  { path = "once_cell::sync::Lazy", reason = "use std::sync::LazyLock" },
  { path = "std::os::fd::RawFd", reason = "use OwnedFd / BorrowedFd" },
]
```

CI: `fmt --check`, `clippy -D warnings`, `test --workspace`, `cargo deny check`, `cargo machete`; Miri job on crates with `unsafe`.

## Things agents reliably get wrong

Check these first when reviewing agent output:

1. **`.clone()` to silence the borrow checker.** One is a smell; a cluster is a design problem — usually ownership was never decided.
2. **`Rc<RefCell<T>>` / `Arc<Mutex<T>>` as the default** for anything shared. Usually a move, a channel, `thread::scope`, or an arena with indices.
3. **Strong `Rc`/`Arc` cycles** (parent ↔ child). Silent leak. Use `Weak` or indices.
4. **Lock guard held across `.await`, I/O, a callback, or a second lock.**
5. **Check-then-act across two lock acquisitions.**
6. **`.lock().unwrap()` with no poisoning policy.**
7. **`Ordering::SeqCst` everywhere, or `Relaxed` on a flag that publishes data.**
8. **Dropped `JoinHandle`** — detached thread, swallowed panic, no shutdown.
9. **Unbounded channel / cache / buffer** fed by external input.
10. **`unwrap()` / indexing / `as` casts on untrusted input** — a crash is a DoS.
11. **`unsafe` without `// SAFETY:`**, or `RawFd` with a manual `close`.
12. **`unsafe impl Send/Sync`** to make an error go away.
13. **`static mut`, `lazy_static!`, `once_cell`** where `LazyLock` / `OnceLock` exist.
14. **`Command::new("sh").arg("-c").arg(format!(..))`** with user data.
15. **Secrets with `#[derive(Debug)]`** — they end up in logs.
16. **`Box<dyn Error>` in libraries**; `anyhow` belongs at bin boundaries.
17. **`async fn` in a trait, then `Box<dyn Trait>`** — not dyn-compatible.
18. **`pub` on everything** to make the compiler stop complaining.
19. **Discarding `Result` with `let _ =`.**
20. **`Vec::push` / `format!` / `String::new()` in hot loops** with no pre-sizing or buffer reuse; unbuffered writes to stdout/files.
21. **Generic explosion** or the reverse (trait objects where a closed enum fits).
22. **Deps without `default-features = false`** (`reqwest`, `tokio`, `chrono`).
23. **Config without `#[serde(deny_unknown_fields)]`.**
24. **Tests that pass because of `sleep`.**
25. **Drop order assumed but not enforced** by scope or explicit `drop`.

## Reference shelf — where these rules come from

When a rule here is unclear or a case isn't covered, go to the source, in this order. All are free online.

**Load into an agent session for a Rust review:**
- **Microsoft Pragmatic Rust Guidelines** — <https://microsoft.github.io/rust-guidelines/>. The single-file agent edition is <https://microsoft.github.io/rust-guidelines/agents/all.txt> (~34k tokens). The `M-*` IDs cited above come from here. Load it for a full review, not for every edit.

**Memory & ownership**
- *The Rust Book* ch. 4 (ownership), 15 (smart pointers, `Weak`, reference cycles), 16 (fearless concurrency) — <https://doc.rust-lang.org/book/>
- *Learning Rust With Entirely Too Many Linked Lists* — why `Rc<RefCell>` graphs hurt, and what to do instead — <https://rust-unofficial.github.io/too-many-lists/>
- `std::os::fd` (`OwnedFd`, `BorrowedFd`, I/O safety) — <https://doc.rust-lang.org/std/os/fd/index.html>

**Thread safety & atomics**
- *Rust Atomics and Locks* (Mara Bos), free online — the reference for memory ordering, `Mutex`/`Condvar` internals, and when `Relaxed` is enough — <https://mara.nl/atomics/>
- `std::sync` module docs (poisoning, `LazyLock`, `OnceLock`, `mpsc`) — <https://doc.rust-lang.org/std/sync/index.html>
- Tokio tutorial — *Shared state* (std `Mutex` in async is fine if never held across `.await`) and *Channels* — <https://tokio.rs/tokio/tutorial/shared-state>
- `loom` (exhaustive interleaving tests) — <https://docs.rs/loom>

**`unsafe` & soundness**
- *The Rustonomicon* — <https://doc.rust-lang.org/nomicon/>; read *Exception safety* before writing a `Drop` or a collection
- *Unsafe Code Guidelines* reference — <https://rust-lang.github.io/unsafe-code-guidelines/>
- Miri — <https://github.com/rust-lang/miri>

**Security**
- **ANSSI Secure Rust Guidelines** (French national cybersecurity agency) — memory, FFI, integer overflow, dependencies; ends in a checklist — <https://anssi-fr.github.io/rust-guide/> · checklist: <https://anssi-fr.github.io/rust-guide/checklist.html>
- RustSec advisory DB (what `cargo audit` / `cargo deny` read) — <https://rustsec.org/>

**Performance**
- *The Rust Performance Book* (Nicholas Nethercote) — profiling, heap allocations, hashing, I/O buffering — <https://nnethercote.github.io/perf-book/>

**API design & maintainability**
- *Rust API Guidelines* (the official checklist) — <https://rust-lang.github.io/api-guidelines/checklist.html>
- *Rust Design Patterns* (idioms + anti-patterns, incl. "clone to satisfy the borrow checker") — <https://rust-unofficial.github.io/patterns/>
- Clippy lint index (check a lint name before adding it) — <https://rust-lang.github.io/rust-clippy/stable/>

## Pattern fit

Which sections are load-bearing depends on the archetype:

- **Workspace / multi-crate tool** — architecture defaults + lint baseline; feature-gate optional parts. Template for new Rust workspaces.
- **CLI + store** — one binary + SQLite; thin `main.rs`, logic in the lib. If it ever goes async, `rusqlite` calls move to `spawn_blocking`.
- **Hand-rolled server** (std threads + epoll/kqueue) — thread safety, `unsafe`, input bounds, `OwnedFd`, `overflow-checks = true`.
- **Async + cryptographic** — async landmines, secrets handling, `overflow-checks = true`.

Web apps: [[sutegi]] is the default framework (std-only, no async runtime) — see [[stack-defaults]].

_Source:_ MS Rust guidelines ingest + lessons from real agent-written Rust; full review set → [[2026-06-10-ms-rust-interop]].

See also: [[query-system-design]].

Back to the [[library]].
