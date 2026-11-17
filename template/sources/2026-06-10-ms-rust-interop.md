---
title: "Microsoft Rust Guidelines — Libraries / Interoperability"
summary: "MS Rust guidelines → [[rust-style-guide]]"
type: source
url: https://microsoft.github.io/rust-guidelines/guidelines/libs/interop/index.html
source: https://github.com/microsoft/rust-guidelines
ingested: 2026-06-10
---

# Libraries / Interoperability Guidelines

## Types are Send (M-TYPES-SEND)

**Why:** To enable the use of types in Tokio and behind runtime abstractions.
**Version:** 1.0

Public types should be `Send` for compatibility reasons:
- All futures produced (explicitly or implicitly) must be `Send`
- Most other types should be `Send`, but there might be exceptions

### Futures

When declaring a future explicitly you should ensure it is, and remains, `Send`.

```rust
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

struct Foo {}

impl Future for Foo {
    // Explicit implementation of `Future` for your type
    type Output = ();
    fn poll(self: Pin&mut Self>, _: &mut Context'_) -> Poll<<Self as Future>::Output> { todo!() }
}

// You should assert your type is `Send`
const fn assert_send<T: Send>() {}
const _: () = assert_send::<Foo>();
```

When returning futures implicitly through `async` method calls, you should make sure these are `Send` too.
You do not have to test every single method, but you should at least validate your main entry points.

```rust,edition2021
async fn foo() { }

fn assert_send<T: Send>(_: T) {}
_ = assert_send(foo());
```

### Regular Types

Most regular types should be `Send`, as they otherwise infect futures turning them `!Send` if held across `.await` points.

```rust,edition2021
use std::rc::Rc;
async fn read_file(x: &str) {}

async fn foo() {
    let rc = Rc::new(123);      // <-- Holding this across an .await point prevents
    read_file("foo.txt").await; //     the future from being `Send`.
    dbg!(rc);
}
```

That said, if the default use of your type is *instantaneous*, and there is no reason for it to be otherwise held across `.await` boundaries, it may be `!Send`.

```rust,edition2021
async fn foo() {
    // Here a hypothetical instance Telemetry is summoned
    // and used ad-hoc. It may be ok for Telemetry to be !Send.
    telemetry().ping(0);
    read_file("foo.txt").await;
    telemetry().ping(1);
}
```

### The Cost of Send

Ideally, there would be abstractions that are `Send` in work-stealing runtimes, and `!Send` in thread-per-core models based on non-atomic types like `Rc` and `RefCell` instead.

Practically these abstractions don't exist, preventing Tokio compatibility in the non-atomic case. That in turn means you would have to "reinvent the world" to get anything done in a thread-per-core universe.

The good news is, in most cases atomics and uncontended locks only have a measurable impact if accessed more frequently than every 64 words or so.

Working with a large `Vec<AtomicUsize>` in a hot loop is a bad idea, but doing the occasional uncontended atomic operation from otherwise thread-per-core async code has no performance impact, but gives you widespread ecosystem compatibility.

---

## Don't Leak External Types (M-DONT-LEAK-TYPES)

**Why:** To prevent accidental breakage and long-term maintenance cost.
**Version:** 0.1

Where possible, you should prefer `std` types in public APIs over types coming from external crates. Exceptions should be carefully considered.

Any type in any public API will become part of that API's contract. Since `std` and constituents are the only crates shipped by default, and since they come with a permanent stability guarantee, their types are the only ones that come without an interoperability risk.

A crate that exposes another crate's type is said to *leak* that type.

For maximal long term stability your crate should, theoretically, not leak any types. Practically, some leakage is unavoidable, sometimes even beneficial. We recommend you follow this heuristic:

- [ ] if you can avoid it, do not leak third-party types
- [ ] if you are part of an umbrella crate, you may freely leak types from sibling crates.
- [ ] behind a relevant feature flag, types may be leaked (e.g., `serde`)
- [ ] without a feature *only* if they give a *substantial benefit*. Most commonly that is interoperability with significant other parts of the Rust ecosystem based around these types.

---

## Native Escape Hatches (M-ESCAPE-HATCHES)

**Why:** To allow users to work around unsupported use cases until alternatives are available.
**Version:** 0.1

Types wrapping native handles should provide `unsafe` escape hatches. In interop scenarios your users might have gotten a native handle from somewhere else, or they might have to pass your wrapped handle over FFI. To enable these use cases you should provide `unsafe` conversion methods.

```rust
type HNATIVE = *const u8;
pub struct Handle(HNATIVE);

impl Handle {
    pub fn new() -> Self {
        // Safely creates handle via API calls
        todo!()
    }

    // Constructs a new Handle from a native handle the user got elsewhere.
    // This method should then also document all safety requirements that
    // must be fulfilled.
    pub unsafe fn from_native(native: HNATIVE) -> Self {
        Self(native)
    }

    // Various extra methods to permanently or temporarily obtain
    // a native handle.
    pub fn into_native(self) -> HNATIVE { self.0 }
    pub fn to_native(&self) -> HNATIVE { self.0 }
}
```
