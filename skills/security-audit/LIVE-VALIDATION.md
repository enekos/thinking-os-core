# Owner-Authorized Live Validation (Phase 3b)

A thinking-os addition; not part of upstream. Source evidence says a path exists.
Only the deployed system says whether an attacker can walk it: a gateway
guard, a proxy rule, a schema change or a fix shipped after your checkout can
close a path the source shows open. Phase 3b replays the minimum attack against
a **non-production** deployment so a finding is reported as exploitable, not as
plausible.

This is the only exception to "do not probe deployed endpoints" in
`SKILL.md`. Everything else in Universal execution safety still applies.

## When it is on

All three are required. Missing any one → Phase 3b does not run, and the
upstream source-and-local-only rules apply unchanged.

1. **Owner request in this session**: "validate against staging", "prove it on
   staging", "check real exploitability". Loading the skill is not a request.
2. **A deployment profile** in the wiki: `concepts/security-live-validation*.md`
   under `$THINKING_OS_ROOT` (fallback `~/thinking-os`). It names the allowed
   environment, the entry interface, how to read the deployed version, how the
   owner hands over sessions, and any per-org limits. Read it whole.
3. **Owner-supplied principals**: an attacker session and a victim
   principal/resource the owner controls (own test tenants). Ask for them;
   never create, guess or harvest credentials.

Record in `run-metadata.json`: `execution_policy:
"sandboxed-local-plus-live:<env>"`, the profile path, the deployed version and
the attacker/victim principal ids (ids only, no secrets).

## Hard limits

- **Never production.** Only the environment the profile names. If the host,
  cluster context or DNS resolves to anything else, stop.
- **Owner's principals only.** The victim is a tenant/user/resource the owner
  named. If a request returns data of any other principal, that already proves
  the boundary: stop, record ids and field *names* only, and do not paginate,
  enumerate or repeat.
- **Never touch authentication controls.** No captcha bypass, credential
  stuffing, brute force, token forging or login scripting. The owner logs in
  and hands over the session value; it lives in an env var for the run, never
  in a file, artifact, transcript or commit.
- **Minimum effect.** One request (or the shortest sequence) that shows the
  crossed boundary. Reads before writes. A write is allowed only against the
  owner's victim resource, with the inverse request prepared first and run
  immediately after. No availability, rate-limit, fuzzing or load tests. No
  paid side effects (emails, SMS, LLM calls, payments, job-board publishing) —
  if the attack path triggers one, stop and leave it `needs_validation`.
- **Sequential and countable.** No concurrency. The profile may set a request
  cap; the default is 10 requests per candidate.
- **Redact on capture.** Transcripts keep method, operation, variables with
  ids, status and response *shape* (field names, counts, ids). Replace every
  email, phone, name, file URL, token and free-text value with `<redacted>`
  before it reaches disk.

## Version pin first

A live check is evidence only about the version that answered it.

1. Read the deployed version the way the profile says (image tag, `/version`
   route, release record).
2. Compare it with the audited `source_ref`:
   `git diff <deployed-tag> <source_ref> -- <every trace path>`.
3. Empty diff → the live result speaks for the audited source. Non-empty →
   re-read the trace at the deployed tag first; a fix that landed in between
   turns the candidate into `rejected` with reason "fixed in <tag>/<sha>", not
   into a live probe.

Auditing a stale checkout is the most common source of false positives. Audit
`origin/<default>` or the deployed tag, never a main checkout of unknown age.

## Who runs it

Hunters stay source-only. The **parent** runs Phase 3b, after Phase 3 and
before Phase 4, for every candidate whose trace starts at an entrypoint the
profile's deployment exposes:

- Phase 3 `confirmed` → live check required before Phase 4.
- Phase 3 `needs_validation` whose blocker is a deployed fact the live check
  can observe → live check.
- `rejected` → none.

Write the exact request(s) to the candidate's verifier `scratch/` first, then
send them from a plain HTTP client (curl, fetch) outside any target-controlled
process. Promote the redacted transcript to that verifier's `artifacts/` with
the promotion procedure in `SKILL.md`, and record the ledger check as
`method: "local"` with that artifact and an invariant beginning `live:<env>:`.

## Outcomes

| Live result | Verdict |
|---|---|
| Boundary crossed as traced | `confirmed`; `execution.observed_result` starts `Observed on <env> <version>:` and states ids/field names only |
| Blocked by a control the source did not show (gateway guard, ingress rule, schema validation, WAF) | `rejected`; `reason` names the control and the response |
| Blocked, but only because the test principals lack a precondition a real attacker could have | `needs_validation`; blocker = that precondition |
| Path removed or fixed in the deployed version | `rejected`, reason "fixed in <version>" |
| Request could not be sent within these limits | keep the Phase 3 verdict; note "not live-validated: <why>" |

A source `confirmed` that fails live is not quietly kept: either name the
missing precondition (`needs_validation`) or reject it.

Severity follows what was observed live. A cross-tenant read observed only
as ids and field names is still a cross-tenant read; do not inflate it to the
worst data the field could hold, and do not deflate it because you stopped
early.

## Phase 5 and reporting

- Phase 5 verifiers check the transcript against the record. They do not
  re-send requests; if the transcript does not prove the claim, demote.
- `REPORT.md` states the environment, deployed version, number of live
  requests sent, and for each confirmed record whether it was live-validated.
  Confirmed records that were not live-validated are listed as such.
- Reports, `findings.json` and transcripts never carry session values or
  victim PII. Share them only after reading them once for leaks.
