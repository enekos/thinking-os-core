---
title: Verification and test design patterns
summary: "risk-based verification with regression proofs, property, metamorphic, differential, model, fuzz, mutation, conformance, compile-fail, concurrency, integration, end-to-end, and performance tests"
type: concept
updated: 2026-11-17
edges:
  - "APPLIES_TO sutegi"
  - "APPLIES_TO osagai"
---

# Verification and test design patterns

Choose an oracle that can disprove the behavior at risk. More tests are not the
goal; the goal is a cheap, deterministic check that fails when a named invariant
breaks. Pair this page with [[rust-type-driven-design]],
[[rust-resource-and-concurrency-patterns]], [[typescript-domain-patterns]], and
[[typescript-async-and-resilience-patterns]].

## Select by failure mode

| Failure at risk | Primary verification | Weak substitute to avoid |
|---|---|---|
| One reported bug returns | minimal regression test with failure proof | retesting only the happy path |
| Many inputs violate one invariant | property-based test | a larger table of hand-picked examples |
| Change must preserve a relation, not exact output | metamorphic test | snapshot of one arbitrary output |
| New implementation must match an oracle | differential test | testing both against their own expectations |
| Legal operation sequences diverge | model-based state-machine test | independent tests of each method |
| Untrusted bytes cause panic/hang | fuzzing with limits and shrinking | one malformed fixture |
| Tests pass when behavior is broken | mutation testing on critical logic | coverage percentage |
| Fake and real adapter disagree | shared conformance suite | mock call-order assertions |
| Invalid API use must not compile | compile-fail/type-level tests | prose saying "do not call" |
| Race or shutdown deadlock | deterministic interleaving/model test | `sleep` and repeated CI reruns |
| SQL, wire, or transaction behavior differs | real adapter integration test | an in-memory approximation only |
| Critical user journey breaks | small browser/system E2E suite | E2E coverage of every branch |
| Optimization regresses throughput/latency | controlled benchmark + distribution | one wall-clock sample |

Layer checks. A property test does not replace a real database transaction
test; an E2E test does not make a domain invariant cheap to diagnose.

## Regression test with failure proof

**Use when.** Fixing a defect or preventing a previously observed incident.

**Invariant.** The test distinguishes the broken implementation from the fix.

1. Minimize the failing input and name the violated rule.
2. Write the test at the lowest layer that exposes the defect.
3. Run it against the unfixed code in an isolated worktree or by temporarily
   reversing only the fix; record the expected failure.
4. Restore the fix and run the same test plus the surrounding suite.
5. Preserve the failing fixture, not an agent transcript.

If the test passes on both versions, it does not verify this fix. If it fails
for an unrelated setup reason, the proof is inconclusive.

**Review check.** The test asserts behavior, not the implementation detail that
happened to cause the bug. For example, assert duplicate delivery creates one
effect, not that a new mutex method was called.

## Property-based testing

**Use when.** A single rule holds for a broad input space.

Good properties:

- Round trip: `decode(encode(value)) == value` for canonical values.
- Conservation: counts/bytes/totals are preserved or bounded.
- Ordering: sorted output is ordered and contains exactly the input elements.
- Idempotence: applying an operation twice equals applying it once.
- Non-interference: unrelated entries remain unchanged.
- Safety: arbitrary input returns an error or value, never panic/hang.

```rust
proptest::proptest! {
    #[test]
    fn canonical_tsv_round_trip(rows in proptest::collection::vec(any_row(), 0..100)) {
        let encoded = encode_rows(&rows);
        proptest::prop_assert_eq!(decode_rows(&encoded), rows);
    }
}
```

Use the repository's Rust (`proptest`, `bolero`) or TypeScript (`fast-check`)
tooling rather than adding a second framework.

- Generate mostly valid values plus adversarial invalid values; purely random
  strings rarely reach a structured parser's interesting branches.
- Bound generated size/depth to the actual input contract.
- Persist the original failing seed and minimized counterexample as a fixture.
- A failing shrinker result is evidence to analyze, not something to loosen
  until green.

**Review check.** The property must not simply call the implementation twice
and compare it with itself unless self-consistency is the actual invariant.

## Metamorphic testing

**Use when.** Exact expected output is difficult, but a relation between inputs
and outputs is known.

Examples:

- Adding unrelated graph nodes does not change a local query result.
- Reordering independent input pages does not change the fused graph.
- Renaming an internal temporary path does not change parsed content.
- Increasing concurrency preserves results while preserving the bound.
- A cache hit returns the same value as a cache miss.

Name the relation and any legitimate exceptions. Timing, random ordering, and
floating-point approximation need explicit equivalence rules.

**Review check.** A metamorphic relation that accepts both old and new behavior
without distinguishing the regression is too broad.

## Differential testing

**Use when.** Replacing a parser, optimizer, storage engine, or algorithm that
has an independent oracle.

```text
same input → old and new implementation → normalize defined differences → compare
```

- The oracle must be independent enough to make divergence informative.
- Define normalization before running: map iteration order, insignificant
  whitespace, timestamps, and explicitly accepted changed behavior.
- Save divergent minimized inputs and classify each as a bug or intended delta.
- Differential success does not prove both implementations correct; they can
  share a wrong assumption.

For Thinking-OS, a rewritten extractor can be compared with its predecessor on
the same fixture corpus. Do not manufacture a Python or JavaScript oracle just
to satisfy this pattern.

## Model-based state-machine testing

**Use when.** Behavior depends on history and legal transitions.

1. Write a deliberately small model with the legal states and commands.
2. Generate command sequences, including invalid commands.
3. Apply each command to model and implementation.
4. Compare state plus the specified result/error.
5. Shrink the sequence and save the shortest divergence.

Model publication as `draft → ready → published`, not three independent method
tests. Include duplicate publish, review after publish, restart/reload, and
cancellation where relevant.

**Review check.** The model must be simpler than the implementation and owned
by the domain rules. Copying implementation branches into the model creates two
equal bugs.

## Golden and snapshot testing

**Use when.** A large stable representation is expensive to assert field by
field and reviewable by a human: rendered help, generated views, serialized
fixtures, compiler diagnostics.

- Keep goldens small, deterministic, and meaningful in review.
- Separate stable content from timestamps, absolute paths, random IDs, and
  platform newline differences.
- Never accept regenerated snapshots without reading the semantic diff.
- Add at least one explicit assertion for the critical invariant; a snapshot
  only records what happened, not why it is correct.
- Store binary/large generated evidence outside prose pages unless the graph
  schema explicitly owns it.

**Review check.** A PR that changes hundreds of snapshot lines without a
corresponding behavioral explanation is not verified.

## Fuzzing untrusted input

**Use when.** Parsers, decoders, file formats, network input, or complex text
processing accept attacker/user-controlled bytes.

- Start from valid seeds and a dictionary of grammar tokens.
- Assert no panic, abort, uncontrolled allocation, excessive depth, or timeout.
- Include semantic assertions such as successful values satisfying invariants.
- Set memory/time limits and run continuously or on a scheduled budget; a short
  PR smoke corpus complements rather than replaces exploration.
- Minimize and commit every discovered crash to the ordinary regression suite.

Rust: `cargo fuzz`/libFuzzer where supported. TypeScript: property-based
generators, coverage-guided tooling where maintained, and explicit input-size
limits. Do not fuzz through a production network service as the first harness.

## Mutation testing for test quality

**Use when.** Critical pure logic has high coverage but weak assertions:
authorization, money, state transitions, duplicate detection, bounds.

- Run mutation testing on a focused module, not as a blanket repository gate.
- Inspect surviving mutants by behavior. A missing assertion, unreachable test
  data, or an intentionally equivalent mutant needs different treatment.
- Add an assertion or input that kills the behavioral mutant. Do not delete
  correct production code merely to satisfy the tool.
- Track killed, timed-out, ignored, and equivalent mutants with reasons.

Coverage identifies unexecuted code; mutation testing asks whether execution is
actually checked. Neither replaces domain reasoning.

## Adapter/fake conformance suite

**Use when.** Two implementations must satisfy one port: memory/SQL store,
real/fake clock, production/test mailer, local/remote filesystem.

Write reusable cases for:

- Empty/not-found behavior and error categories.
- Ordering, pagination boundaries, and duplicate handling.
- Atomic writes, transaction rollback, and concurrent claims.
- Cancellation and resource cleanup where part of the port.
- Timezone, precision, and deterministic clock semantics.

Run the suite against the real infrastructure in integration CI and the fake in
unit tests. If a fake intentionally cannot support a behavior, narrow the port
or mark the difference explicitly; never silently skip the hard case.

**Review check.** Use-case tests assert final state and outputs. A fake that
only records expected method calls tests the current implementation shape.

## Compile-fail and API verification

**Use when.** An API's illegal use is a compile error by design.

Rust:

- `trybuild` passing and failing crates for typestate/capability boundaries.
- `static_assertions` or local const assertions for `Send`/`Sync`/size contracts.
- `cargo public-api diff` for intentional public surface changes.
- `cargo semver-checks` where the crate's supported baseline is configured.

TypeScript:

- `tsc --noEmit` consumer fixtures using the package's exported declarations.
- `expect-type`, `tsd`, or `expectTypeOf` positive and negative assertions.
- `@ts-expect-error` on exact invalid expressions, never as a blanket silence.

A compile-fail test needs a positive counterpart. Otherwise deleting the API or
breaking every import can make the negative suite appear green.

## Concurrency and failure injection

**Use when.** Correctness depends on ordering, interruption, or crashes.

- Replace sleeps with barriers, channels, deferred promises, fake clocks, or an
  interleaving explorer.
- Rust custom synchronization: reduced `loom` model; unsafe memory: Miri.
- JavaScript async: deferred promises let the test choose every settlement
  order. Assert listener/task cleanup after all paths.
- Inject failure after each durable step, not just before an operation starts.
- Define and assert the terminal state after cancellation, timeout, and restart.
- Run stress tests as an additional detector, never as proof of race freedom.

See the language pattern pages for the specific lifecycle invariants.

## Integration and end-to-end tests

**Integration** uses the real component whose behavior is being checked: real
SQL for a repository, real filesystem semantics for atomic replacement, a real
HTTP server for routing and content handling. Isolate each run/worker and make
cleanup reliable.

**E2E** covers a small number of critical user/system journeys with seeded data
and observable outcomes. Browser tests locate by accessible role/label and wait
on conditions, not arbitrary timeouts.

Do not mock the component under test. Do not place every domain branch in E2E;
that makes failures slow and ambiguous.

## Performance regression verification

**Use when.** A change claims an optimization or touches a known hot path.

- Record hardware/runtime, input corpus, warm-up, iteration count, and metric.
- Compare distributions or confidence intervals, not a single fastest run.
- Include latency tails and allocation/queue growth when they matter.
- Keep correctness checks active under the optimized path.
- Separate microbenchmarks from user-level load tests; each answers a different
  question.
- Treat environmental noise as inconclusive, not as a regression or improvement.

Use the project's established benchmark harness and [[autoresearch]] for
iterative keep/revert experiments.

## Flakes, quarantine, and coverage policy

- A flaky test is a failing quality signal. Minimize and fix the product or
  test; do not normalize rerunning until green.
- Quarantine only with an owner, issue, captured failure, and expiry. Keep the
  failure visible and prevent new dependents from treating quarantine as proof.
- Do not require an arbitrary global coverage threshold. Require named tests
  for critical invariants, changed behavior, and bug fixes.
- Never weaken an assertion, increase a sleep, or broaden accepted output merely
  to remove a failure without evidence that the specification changed.

## Thinking-OS verification map

| Area | High-value checks |
|---|---|
| frontmatter/YAML extraction | malformed corpus, round-trip properties, fuzzing |
| typed edges and fusion | domain/range cases, graph invariants, order-independence properties |
| duplicate poll/event application | idempotence property and sequential model |
| atomic file writes | real-filesystem integration and write-failure injection |
| retype/migration | golden views plus explicit no-data-loss assertions |
| GitHub CLI parsing | malformed/empty/large JSON fixtures and timeout behavior |
| viewer/API | focused integration tests; browser E2E only for critical viewing flows |

## Recording evidence

For code-aware wikis, a `tests/` page records one exact command at one exact
commit and its exit code. A green run earns `VERIFIES` only for the commit it
actually ran against. Do not record unexecuted recommendations as passed tests.
Failed and inconclusive runs remain valuable evidence; see
[[architecture-fitness-and-quality-gates]] for gate selection and reporting.

## Review checklist

- Which invariant can this check disprove?
- Has a regression test been shown to fail without the fix?
- Is the oracle independent, deterministic, and sensitive to the risk?
- Are generators bounded and failures minimized into durable fixtures?
- Does any test rely on sleep, shared global state, or rerun-to-green?
- Are expensive techniques targeted at the behavior that justifies their cost?

_Provenance:_ house verification practice grounded in property-based testing,
model checking, fuzzing, and mutation-testing literature. No immutable source
has been ingested for this page yet. Project tools and exact versions remain a
repository decision; see also [[eval-methods]] for evaluating LLM/agent outputs.

Back to [[library]].
