---
title: Rust type-driven design patterns
summary: "advanced Rust API patterns for making invalid states unrepresentable: validated newtypes, enum state machines, typestate, capabilities, sealed traits, dispatch choices, and compile-time verification"
type: concept
updated: 2026-11-17
edges:
  - "APPLIES_TO sutegi"
---

# Rust type-driven design patterns

Push a rule into the type system when misuse is common, costly, or hard to test.
Do not encode every runtime fact in a type: the goal is fewer invalid programs,
not a larger type puzzle. General Rust hygiene stays in [[rust-style-guide]];
verification techniques are in [[verification-patterns]].

## Pattern contract

Every pattern below names its design pressure, invariant, simplest form,
trade-off, and checks. Prefer the weakest mechanism that enforces the rule:

| Rule to enforce | Reach for | Avoid |
|---|---|---|
| Two strings/integers must not be interchangeable | newtype | aliases such as `type UserId = String` |
| Construction must validate | private fields + `TryFrom`/`FromStr` | public fields followed by `validate()` |
| Exactly one runtime state is active | enum with state-owned data | flags plus optional state fields |
| Call order is fixed and misuse is expensive | consuming typestate transitions | typestate for an ordinary CRUD struct |
| An operation requires proof obtained elsewhere | capability token | a boolean such as `authorized: bool` |
| A trait's implementations must stay closed | sealed trait | sealing every public trait by default |
| The variant set is closed | enum | a plugin registry and trait objects |
| The variant set is open and heterogeneous | `dyn Trait` | generic parameters on every containing type |

## Validated newtype

**Use when.** A primitive has domain meaning or an invariant. `NodeId` and
`CommitId` should not be interchangeable merely because both contain strings.

**Invariant.** No safe public operation constructs an invalid value.

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct NodeId(String);

#[derive(Debug, thiserror::Error)]
#[error("invalid node id: {0}")]
pub struct ParseNodeIdError(String);

impl std::str::FromStr for NodeId {
    type Err = ParseNodeIdError;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        let valid = !raw.is_empty()
            && raw.len() <= 128
            && raw.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-');
        if valid {
            Ok(Self(raw.to_owned()))
        } else {
            Err(ParseNodeIdError(raw.to_owned()))
        }
    }
}

impl std::fmt::Display for NodeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
```

- Keep the field private. `AsRef<str>` and `Display` expose reads without
  exposing construction.
- Implement `TryFrom<&str>`/`FromStr`, not an unrelated `parse_node_id` helper.
- Deriving `Deserialize` on a tuple newtype bypasses `FromStr`. Deserialize via
  a string followed by validation, or use aSerde remote/helper that does so.
- A newtype adds no semantic validation by itself. A UUID wrapper accepts any
  UUID; `NodeId::from_str` owns the node grammar.
- Do not implement `Deref<Target = str>` to fake inheritance. It exposes the
  entire underlying API and weakens the abstraction.

**Checks.** Unit-test malformed, empty, oversized, Unicode, and boundary input.
Property-test `parse(id.to_string()) == id`. Fuzz the parser when the input is
untrusted. Test serialization separately: a hand-written constructor does not
prove deserialization validates.

**Avoid when.** The wrapper is passed straight through with no invariant and no
possibility of confusion. A local function argument can remain a primitive.

## Enum state machine

**Use when.** State is chosen at runtime and each state owns different data.

**Invariant.** Data that belongs to one state cannot exist in another.

```rust
enum Publication {
    Draft,
    Ready { reviewed_by: String },
    Published { reviewed_by: String, published_at: std::time::SystemTime },
}

impl Publication {
    fn publish(self, at: std::time::SystemTime) -> Result<Self, PublishError> {
        match self {
            Self::Ready { reviewed_by } => Ok(Self::Published { reviewed_by, published_at: at }),
            Self::Draft => Err(PublishError::NotReviewed),
            Self::Published { .. } => Err(PublishError::AlreadyPublished),
        }
    }
}
```

- Put state data in the variant, not in sibling `Option` fields.
- Make transition methods consume `self` when the old state must no longer be
  usable. Use `&mut self` only when callers may retain the same logical object.
- Match exhaustively inside the crate. Add a new variant and let the compiler
  list every behavior that needs a decision.
- Keep serialization state separate if persisted data can be legacy or
  incomplete; validate it into this enum at the boundary.

**Checks.** Model-test legal and illegal transition sequences. Assert the exact
error on an illegal transition and the exact state-owned data after success.

**Anti-pattern.** `draft: bool`, `reviewed_by: Option<String>`, and
`published_at: Option<SystemTime>` permit published-without-review and other
states the prose says are impossible.

## Typestate protocol

**Use when.** A small protocol has a fixed call order, and violating it is a
programming error rather than runtime input. Good fits include a connection
handshake or a builder with one required field.

**Invariant.** An operation is callable only in a state that supplies its
requirements.

```rust
struct MissingUrl;
struct Ready { url: String }

struct RequestBuilder<S> {
    state: S,
    headers: Vec<(String, String)>,
}

impl RequestBuilder<MissingUrl> {
    fn new() -> Self {
        Self { state: MissingUrl, headers: Vec::new() }
    }

    fn url(self, url: String) -> RequestBuilder<Ready> {
        RequestBuilder { state: Ready { url }, headers: self.headers }
    }
}

impl<S> RequestBuilder<S> {
    fn header(mut self, name: String, value: String) -> Self {
        self.headers.push((name, value));
        self
    }
}

impl RequestBuilder<Ready> {
    fn build(self) -> Request {
        Request { url: self.state.url, headers: self.headers }
    }
}
```

The ready state owns the URL; `build` has no hidden `Option::unwrap`.
Transitions consume `self`, so the old incomplete builder cannot be reused.

**Checks.** Use `trybuild` for negative examples: `build` before `url`, and use
of a consumed builder, must fail. Add ordinary passing cases as well; a broken
API where everything fails is not a successful compile-fail suite. Keep expected
compiler output current, but assert failure rather than blessing unrelated text.

**Avoid when.** States come from input, transitions are numerous, or every
method needs a different combination of type parameters. Use an enum instead.
Typestate multiplies public types, documentation, compile errors, and sometimes
monomorphization.

## Capability token

**Use when.** One component establishes proof and another should be unable to
perform the protected operation without receiving it.

```rust
pub struct WriteCapability {
    _private: (),
}

impl WriteCapability {
    pub(crate) fn after_authorization() -> Self {
        Self { _private: () }
    }
}

pub fn rename_node(cap: &WriteCapability, id: &NodeId, name: &str) -> Result<(), StoreError> {
    let _ = cap;
    rename_in_store(id, name)
}
```

- The private field prevents safe downstream construction.
- Capability-based APIs make dependencies visible and tests explicit.
- A token is not automatically an authorization boundary. Authorization still
  needs the correct subject, resource, expiry, and revocation semantics.
- Do not make capabilities global or freely cloneable when ownership is meant
  to be restrictive.

**Checks.** Compile-fail construction outside the authorized module. Test that
expired/revoked authority is rejected by the component that issues the token.

## Sealed and extension traits

**Use a sealed trait when.** The trait is public behavior, but downstream
implementations would freeze design decisions or violate crate invariants.

```rust
mod sealed {
    pub trait Sealed {}
}

pub trait GraphId: sealed::Sealed + std::fmt::Display {}
```

Implement `sealed::Sealed` only for approved local types. Sealing prevents
downstream implementations; it does not by itself make adding required trait
methods non-breaking for callers. Re-export the public trait, not the sealing
module.

**Use an extension trait when.** A coherent set of convenience methods belongs
on a foreign type and the orphan rules permit the trait implementation. Do not
use one to hide important dependencies or create import-time magic.

**Checks.** Compile-fail a downstream implementation for a sealed trait. Run
`cargo public-api diff` before releasing a public crate.

## Dispatch: enum, generic, or trait object

| Shape | Choose when | Cost to watch |
|---|---|---|
| `enum Backend { File(FileStore), Sql(SqlStore) }` | variants are closed and callers need one value | every operation needs exhaustive dispatch |
| `fn run<S: Store>(store: &S)` | one concrete type per call site; static dispatch matters | monomorphization and type propagation |
| `Box<dyn Store>` / `Arc<dyn Store>` | heterogeneous collections, plugins, open implementations | allocation, dynamic dispatch, object safety |
| function pointer or closure | one operation, not a whole interface | captures/lifetimes can obscure ownership |

Do not start with `dyn Trait` to make an imagined test double possible. Start
with a concrete dependency; introduce a trait when a second implementation or a
real test boundary exists. Check object safety before committing a public trait
to `dyn`: generic methods and by-value `self` commonly change the design.

## Public invariants and evolution

- Prefer `#[must_use]` on guards, builders, and fallible operations whose result
  must not be ignored.
- Use `#[non_exhaustive]` where downstream matching or struct literals would
  otherwise prevent compatible growth. It is not a substitute for semantic
  versioning.
- Assert important auto-trait contracts explicitly:

```rust
const fn assert_send_sync<T: Send + Sync>() {}
const _: () = assert_send_sync::<NodeId>();
```

- Use `#[expect(lint, reason = "why this exception is necessary")]` rather than
  a permanent `allow`. Remove the exception when the lint stops firing.

## Thinking-OS worked examples

- `tools/src/kg/model.rs`: candidate validated newtypes are node kinds, edge
  relations, and commit IDs. Introduce one only where the ontology or parser
  already enforces a real invariant; do not wrap every string in one pass.
- Extraction and fusion: keep raw deserialized frontmatter separate from an
  ontology-validated page. A `ValidatedPage` type is justified only after the
  validator is the sole constructor.
- `zaindu` PR transitions: model mutually exclusive transition payloads with an
  enum. A runtime event stream should not become a compile-time typestate.

## Review checklist

- Can safe code construct a value that violates its documented invariant?
- Does deserialization use the same validation as ordinary construction?
- Is each `Option` independent, or is it compensating for a missing enum?
- Does every typestate correspond to a real protocol requirement?
- Are negative API uses covered by compile-fail tests?
- Is the abstraction simpler at the call site than the code it replaces?

_Provenance:_ house patterns grounded in the Rust API Guidelines, Rust Design
Patterns, and real agent-review failures. No immutable source has been ingested
for this page yet; references remain links rather than `EVIDENCED_BY` claims:
<https://rust-lang.github.io/api-guidelines/>,
<https://rust-unofficial.github.io/patterns/>,
<https://github.com/rust-lang/rust/blob/master/src/doc/rustdoc/src/write-documentation/what-to-include.md>.

Back to [[rust-style-guide]] · [[library]].
