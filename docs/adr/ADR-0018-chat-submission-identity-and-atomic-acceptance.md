# ADR-0018: Chat Submission Identity And Atomic Acceptance

## Metadata [Required]

- **Decision Status**: Accepted
- **Implementation Status**: In Progress
- **Date**: 2026-10-09
- **Author**: @codex
- **Decision Owner**: @linhai
- **Required Approver**: @linhai
- **Record Scope**: Project
- **Approver [Conditionally Required — Decision Status is or has been `Accepted`]**: @linhai
- **Approval Time [Conditionally Required — Decision Status is or has been `Accepted`]**: 2026-10-09T14:26:36Z
- **Approval Evidence [Conditionally Required — Decision Status is or has been `Accepted`]**: Approve
- **Rejector [Conditionally Required — Decision Status is `Rejected`]**: N/A — this proposal has not been rejected
- **Rejection Time [Conditionally Required — Decision Status is `Rejected`]**: N/A — this proposal has not been rejected
- **Rejection Evidence [Conditionally Required — Decision Status is `Rejected`]**: N/A — this proposal has not been rejected
- **Rejection Reason [Conditionally Required — Decision Status is `Rejected`]**: N/A — this proposal has not been rejected
- **Retired By [Conditionally Required — Decision Status is `Deprecated` or `Superseded`]**: N/A — this proposal has not been retired
- **Retirement Time [Conditionally Required — Decision Status is `Deprecated` or `Superseded`]**: N/A — this proposal has not been retired
- **Retirement Evidence [Conditionally Required — Decision Status is `Deprecated` or `Superseded`]**: N/A — this proposal has not been retired
- **Retirement Reason [Conditionally Required — Decision Status is `Deprecated` or `Superseded`]**: N/A — this proposal has not been retired
- **Blocked From [Conditionally Required — Implementation Status is `Blocked`]**: N/A — implementation has not started
- **Blocker And Evidence [Conditionally Required — Implementation Status is `Blocked`]**: N/A — implementation has not started
- **Blocker Owner [Conditionally Required — Implementation Status is `Blocked`]**: N/A — implementation has not started
- **Blocker Exit Or Recheck Criterion [Conditionally Required — Implementation Status is `Blocked`]**: N/A — implementation has not started
- **Related [Optional]**: [Trello requirement](https://trello.com/c/4WI4sszw); `docs/adr/ADR-0001-provider-neutral-turn-kernel.md`; `docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md`; `docs/adr/ADR-0005-provider-delta-coalescing-and-512-item-turn-budget.md`; `koduck-ai/docs/adr/ADR-0002-typed-http-wire-serialization.md`; `koduck-ai/docs/adr/ADR-0003-correction-item-schema-and-raw-replay.md`; `koduck-ai/docs/adr/ADR-0004-authenticated-correction-admission.md`; `koduck-ai/docs/adr/ADR-0005-effective-correction-projection.md`; `koduck-ai/docs/adr/ADR-0006-effective-provider-context-integration.md`; `docs/adr/ADR-0002-required-ai-ci-postgres-verification.md`; `docs/adr/ADR-0017-push-boundary-sonarqube-verification.md`
- **Architecture Source [Conditionally Required — product demand]**: `docs/architecture/ADD-0001-ai-service-codex-alignment.md` — CAND-18
- **Supersedes [Conditionally Required — this ADR replaces another]**: None
- **Superseded By [Conditionally Required — this ADR is replaced]**: None

## Requirement Level Legend [Required]

- **`[Required]`**: The section or field always applies and MUST remain present
  with complete, verifiable content. Use `None — <reason>` only when the
  template explicitly permits an empty result; never leave it blank.
- **`[Conditionally Required — <trigger>]`**: The section or field MUST be
  completed when its stated trigger applies. When the trigger does not apply,
  retain `N/A — <reason>` unless the template explicitly instructs removal or
  retention as inactive future-lifecycle guidance. A missing trigger assessment
  is incomplete content.
- **`[Optional]`**: The section may be removed without affecting acceptance,
  execution, completion, or verification. If retained, it MUST be accurate and
  complete; optional content MUST NOT substitute for required evidence.

Unlabeled fields inside a `[Required]` section are required.

## Context And Problem Statement [Required]

The Current ADD selects CAND-18: repeated authenticated chat submissions must
identify the same accepted Turn, distinguish changed input, and reconcile an
uncertain acknowledgement without creating another Turn. CAND-1 is Complete,
so the candidate's prerequisite is satisfied. All Full and Lightweight ADRs in
the index were terminal before this proposal was numbered; the Blocked OCR
does not participate in ADR serialization.

At source revision `a0f81b54ca13eb8133c364caf1edb3dfa1c6c96d`, each call to
`SqlxPostgresExecutor::accept_initial` generates a fresh Turn and input Item.
Its existing reconciliation proves that call's generated Item committed, but
a later HTTP request has none of those private identities. Losing the HTTP
response and repeating the request can therefore create another Turn and
another provider invocation. A lease protects an accepted owner; it does not
identify a client's repeated submission.

This proposal gives the client an optional stable submission UUID and makes
its durable binding part of the initial acceptance transaction. Only the
invocation that creates the binding receives execution authority. A retry
receives a bounded acceptance receipt, including after the original Turn has
finished; it neither restarts inference nor subscribes to the original stream.
This is one C-6 implementation slice with necessary C-1/C-2 plumbing in one
implementation PR. It is a project Full ADR because it extends the publicly
consumed REST/SSE request and response contract and adds durable data behavior.

### Input Audit [Required]

| Input | Evidence and confidence | Use and limitation |
| --- | --- | --- |
| Approved outcome and boundary | Current ADD, CAND-18 and R-2; CAND-1 Complete | Authorizes candidate selection, not source implementation before ADR acceptance |
| Existing runtime behavior | The source revision above; stable touchpoints below | Confirms per-call reconciliation and existing stream-header negotiation; supplies no deployed-system or load evidence |
| Existing contracts | Accepted project ADR-0001/0003/0005 and service ADR-0002/0003/0004/0005/0006, with exact paths in Related and traceability | Preserve typed wire serialization, trust/leases, correction representation/admission/projection, canonical replay, resource limits and fresh context preparation |
| Demand coordination | Trello card 4WI4sszw read in the preceding recommendation on 2026-10-09; one research card in the In Progress list | No new card requirement, approval, ownership change, or Trello write is inferred; the captured ADD baseline is unchanged |
| New wire and retention choices | SI contract below and Q-2/Q-3/Q-4 | Product/retention proposals awaiting @linhai's recorded determination before Approve; not settled requirements or deployed behavior |
| Commit-fault evidence design | PostgreSQL deferred-trigger documentation and local locked SQLx transaction source inspection; AC-0 Not Started | Supports a proposed fixture, not proof that dropped COMMIT futures leave committed state; preflight must pass before T-1 |
| Timing and pool assembly | Existing runner constructors, runtime::run, C-5 ActionDeadline, instant-based coalescer and CorruptFixture inspected at the source revision above | No reusable submission clock exists; C-5's 30 s value is a different policy. Production uses PgPoolOptions::new(); the CAND-11 harness overrides pool size, so AC-0 must explicitly match production options and observe actual backend PIDs |

## Scope [Required]

In scope:

- Optional `submission_id` on both existing chat routes, exact request
  comparison, typed acceptance receipts/conflicts, and an implementation
  contract copy after this ADR is Accepted.
- Subject-scoped durable submission identity, one atomic initial acceptance,
  concurrent winner selection, bounded commit reconciliation, and structural
  integrity checks over the original canonical input.
- Consumer-owned monotonic acceptance-clock injection and per-request budget
  values, with system-clock production assembly and deterministic policy tests.
- A distinct created-owner result and retry receipt; early owned lookup and
  authoritative transaction recheck, with existing fresh context preparation
  between them.
- One additive migration and startup registration; focused production-boundary
  tests and the necessary adaptations of existing ports and test doubles.

Out of scope:

- CAND-17 Thread-wide mutation admission, distributed execution ownership,
  new correction/fork routes, background takeover, or exactly-once effects
  across arbitrary distinct submissions.
- Provider implementation, compaction, projection policy, Memory, extensions,
  UI, new observation/replay endpoints, or live retry-stream subscription.
- New dependencies, runtime settings, CI or Sonar coverage configuration,
  builds, releases, deployment, or remote writes.
- Backfilling guessed client identities onto historical Turns, expiring live
  submission bindings, or creating a new data-deletion operation.

## Tensions, Constraints, And Open Questions [Required]

### Identified Tensions [Conditionally Required — competing goals or trade-offs exist]

| ID | Tension | Impact | Decision |
| --- | --- | --- | --- |
| TN-1 | Client retry safety versus existing callers without an identity | Requiring a new field breaks current callers; generating it server-side cannot deduplicate separate requests | Make a supplied UUID opt into the guarantee; omission retains existing fresh-submission behavior |
| TN-2 | Durable acceptance versus inference completion | Returning an accepted identity can be mistaken for completed inference or a new execution grant | Return a distinct constant acceptance receipt for every retry; never return an owner token from a different invocation |
| TN-3 | Exact comparison versus duplicated sensitive content | A hash-only match can hide collision assumptions; another full input copy increases retained data | Compare the immutable sequence-1 canonical input and the originally requested Thread selector; store no second input copy |
| TN-4 | Reconciliation versus bounded waiting | A timed-out commit may have succeeded, but repeated creation or truncating proof is unsafe | Ten-second acceptance deadline; start a write only with at least four seconds remaining, then give write/reconciliation their full two-second attempts; proven Created always enters existing execution or durable cancellation |

### Constraints [Required]

- Keep the Current ADD's CAND-18 outcome, boundary, dependencies, and acceptance
  context unchanged. CAND-17 remains a separate Ready candidate; its Thread-wide
  mutation admission is unaffected by the acceptance deadline.
- Extend only the chat request field set and the identified-submission retry
  outcome in project ADR-0001's authoritative v1 contract. All other existing
  REST/SSE shapes, error bodies, interrupt/approval semantics, and no-identity
  behavior remain authoritative. This is an additive decision, not retirement
  of those Accepted records.
- Preserve authenticated tenant/subject ownership, 65,536 input bytes,
  4,096 raw prior Items/1,048,576 canonical payload bytes, output budgets,
  durable-before-visible ordering, and existing lease fencing/recovery.
- A fresh submission still prepares the CAND-13 effective context before
  atomic acceptance. An existing submission is an observation and need not
  prepare a new model context. There is no new context freshness guarantee
  across distinct simultaneous submissions before CAND-17.
- Use owned application/domain values at consuming ports; SQLx, Axum, JSON
  documents, and database locks remain in adapters. Reuse UUID, typed wire
  serialization, strict JSON validation, existing settlement, and migrated
  PostgreSQL test capability; add no dependency or alternate runtime store.
- The acceptance deadline is part of C-6 bounded atomic acceptance and keeps
  SI-06's settlement conclusions truthful. Runner clock wiring is limited C-2
  support, not an independently deliverable Thread mutation-admission policy.
  Its system implementation uses `std::time::Instant`; its manual
  implementation exists only in tests. This minimal internal composition API
  is in scope. The prohibition on a new production **fault** API/feature means
  no switch, callback or transport field for forcing database errors or commit
  loss; it does not prohibit the clock needed for bounded atomic acceptance.
  Database/lease/recovery timing retains its existing real clock and deadlines.
- Keep new implementation in focused submission modules. At the inspected
  revision, `ports.rs` has 744 physical lines, `postgres.rs` 712,
  `sqlx_executor.rs` 712, and `runtime/mod.rs` 699. These are review context,
  not source-to-ADR equality assertions. Keep parent edits to declarations,
  forwarding, and necessary control branching; review affected files above
  600 lines and introduce no production file above 800 lines. New production
  files stay at or below 600 lines; test files above 1,000 lines require
  decomposition review and none may exceed 1,800 without reapproval.
- Every affected executable unit stays within the non-waivable 80-line limit;
  review above 60 lines. No engineering exception is granted. No configured
  cyclomatic-complexity tool was identified; final executable span and nesting
  review supply the substitute evidence.
- Public declarations receive intent-bearing documentation. Tests assert
  typed outcomes, parsed wire documents, database rows, and authority/effect
  outcomes, not ordinary prose, physical layout, or source line counts.
- The current `tools/sonarqube/rust-coverage.sh` selects only
  `cand_11_correction_admission`, `cand_12_projection`, and `postgres_cand_11`.
  Put AC-5's primary real commit-loss/rollback evidence in `postgres_cand_11`,
  using a disposable-database deferred constraint trigger as specified below.
  This executes the production settlement/reconciliation branches in the
  existing LCOV selection; colocated `--lib` fault tests are supplementary and
  do not contribute to that report. Actual 80% changed-line coverage and push
  Sonar push admission remain required. No configuration expansion is planned as a
  fallback: it could require a further Full ADR while this record is non-terminal
  and would be subject to serialization/Blocked-exception rules. A failed
  fixture preflight or gate needs a recorded disposition and governed design
  revision before continuing, not an unplanned config edit. This ADR grants no
  coverage waiver, test feature, production database-fault API, or configuration
  edit. The acceptance-clock composition API described above is in scope.

### Open Questions [Conditionally Required — material questions exist or were resolved during drafting]

| ID | Question | Owner | Due | Status | Resolution and Evidence |
| --- | --- | --- | --- | --- | --- |
| Q-1 | Does a key authorize another attempt to execute an existing Turn? | @codex | 2026-10-09 | Resolved | No: the ADD's submission invariant explicitly denies execution authority to an observer. SI-04 and SI-06 separate the receipt from the original creator's lease-bearing result |
| Q-2 | Must adopting client identity break callers that omit it? | @linhai | 2026-10-09 | Resolved | @linhai recorded at 2026-10-09T14:26:36Z in the governing conversation: accepted the proposed resolution — optional opt-in under SI-01a, preserving unidentified chat. The compatibility/guarantee boundary is accepted |
| Q-3 | Does "same identity/outcome" require output recovery, or may a retry return only an acceptance receipt? | @linhai | 2026-10-09 | Resolved | @linhai recorded at 2026-10-09T14:26:36Z in the governing conversation: accepted the proposed resolution — SI-05's 202 receipt for active and terminal Turns, without output replay or stream resubscription; the response-loss limitation is accepted and owned observation/replay delivery is deferred to the new ADD candidate proposed in Consequences Mitigations |
| Q-4 | How long must a submission binding remain, and what deletion policy applies? | @linhai | 2026-10-09 | Resolved | @linhai recorded at 2026-10-09T14:26:36Z in the governing conversation: accepted the proposed resolution — SI-08 retains metadata for the canonical Turn's lifetime, with no independent TTL or cleanup and no new deletion capability; this retention policy is accepted without establishing an absolute lifetime or a future deletion design |

Q-2, Q-3, and Q-4 were resolved on 2026-10-09: @linhai recorded each
determination in the governing conversation at 2026-10-09T14:26:36Z, accepting
the proposed resolutions above, and then responded `Approve` for this record in
the same context, which unambiguously identified
`docs/adr/ADR-0018-chat-submission-identity-and-atomic-acceptance.md`. The
author/drafting agent did not approve. This ADR is therefore `Accepted` under
AGENTS.md's canonical Approval and Status contract, and the Submission Contract
below is authoritative.

## Decision Drivers [Required]

1. **P1 — prevent duplicate work**: response loss and overlapping identical
   submissions are realistic retry triggers; the invariant must hold at the
   database boundary across independent runtime handles.
2. **P1 — preserve identity isolation**: a guessed UUID must reveal neither
   another subject's input nor its accepted Turn or execution authority.
3. **P1 — truthful uncertain outcomes**: a committed-but-unacknowledged write
   must be reconciled, while an unproven outcome must remain unavailable.
4. **P2 — bounded operation and compatibility**: lock waits, disconnects, and
   storage failures must not create unbounded retries, duplicate input copies,
   new background jobs, or an incompatible requirement for existing callers.

## Options Considered [Required]

### Option A: Durable subject-scoped identity in atomic Turn acceptance

Bind an optional client UUID, original selector, and accepted Turn in the same
PostgreSQL transaction as the canonical input and initial lease. Separate
new-owner and observed-existing outcomes.

Pros:

- Survives process restart and acknowledgement loss; database uniqueness owns
  the invariant rather than an application cache.
- Uses the existing canonical input as exact comparison evidence.

Cons:

- Requires one migration and explicit retry wire semantics.
- A client must keep its UUID and original input stable; unidentified requests
  still have no cross-request deduplication guarantee.

### Option B: Process-local cache of request digests and results

Pros:

- Avoids a migration and can recognize repeats within one process lifetime.

Cons:

- Eviction or process loss forgets accepted work; concurrent clones can race.
- Content equality alone cannot distinguish an intentional new message from a
  retry and does not satisfy CAND-18's durable operation identity.

### Option C: Reuse client identity as the Turn ID and always execute it

Pros:

- Fewer identity types and a superficially simpler request path.

Cons:

- Conflates lookup with lease-bearing execution authority and can duplicate
  provider or Tool work after acceptance.
- Leaves initial Thread selection, exact input drift, and uncertain commit
  handling without one atomic acceptance contract.

## Decision [Required]

**Selected option**: Option A.

**Rationale**: Submission identity belongs to C-6 because the failure boundary
is the same transaction that creates a Turn. The application only branches on
an owned result. Durable receipts are small and independent of a model's
completion, while a private creator-operation identity distinguishes this
invocation's commit reconciliation from another caller's retry.

### Consequences [Required]

Positive:

- Identical identified submissions converge to one durable Turn and one
  originating execution workflow.
- Response loss does not require clients to infer whether acceptance happened.

Negative:

- Retried synchronous and streaming requests return an acceptance receipt,
  including when the Turn is terminal; they do not reproduce inference output.
- One binding per identified Turn remains as canonical metadata, and existing
  callers gain the guarantee only when they supply a stable identity.
- After losing the original response or stream, a client can recover acceptance
  identity but cannot retrieve the lost model output through this version's
  public API. Current routes expose chat, stream, interrupt, and approvals;
  there is no owned Turn read/replay endpoint. Canonical output remains stored,
  but server-side raw replay capability does not itself provide client access.

Mitigations:

- Specify response media type/status explicitly and test the real Axum
  pre-stream decision path; clients must distinguish acceptance from completion.
- Compare original input without storing a second content copy, prohibit
  automatic expiry, and preserve all existing inference/stop behavior.
- Q-3 proposes a new ADD candidate, **Owned Turn Observation And Raw Replay
  Delivery**, for bounded authenticated status/output reads from canonical
  history, with CAND-3's replay foundation and CAND-18's accepted identity as
  inputs. @linhai must determine the follow-up and acceptance context. No current
  candidate supplies that endpoint; this proposal has no assigned candidate ID,
  Ready status, or implementation authority. Adding it to the Current ADD
  requires a separate approval-invalidating ADD revision and approval. This
  drafting correction does not modify the ADD or promise a delivery date.

### Submission Contract [Required]

This ADR is `Accepted`; this subsection is authoritative and binding for
implementation. The
contract copy to be added during implementation is evidence, not a second
decision authority. `SI-01` through `SI-10` name contract families; the lettered
IDs below are individually traceable clauses where a family is subdivided.

**SI-01a — Optional submission identity.** Both `POST /api/v1/ai/chat` and
`POST /api/v1/ai/chat/stream` accept the existing `input` and optional
`thread_id`, plus optional `submission_id`. A present `submission_id` MUST be
a non-nil UUID encoded as exactly 36 ASCII characters in hyphenated form;
hexadecimal case is ignored when parsing the UUID. Omission opts out of
deduplication; explicit null, an empty string, other types, nil, or other UUID
forms MUST return `400 invalid-request` without a history operation. Parsing
MUST distinguish an absent member from a present null rather than using plain
`Option<String>` deserialization, which collapses those states.

**SI-01b — Strict body and input bounds.** Duplicate members, unknown members,
malformed JSON, empty input, or input above 65,536 UTF-8 bytes MUST return
`400 invalid-request` without a history operation.

**SI-01c — Authentication boundary.** Missing/invalid authenticated identity
MUST return the existing `401 invalid-identity` with Bearer challenge before
a submission lookup, provider call, or history write.

**SI-01d — Direct acceptance guard.** Direct acceptance-port entry points MUST
independently validate the owned
input byte bound and submission identifier before database I/O; an invalid
command cannot bypass the guard by constructing public fields directly. No
adapter or default port implementation may silently ignore a supplied key.

**SI-01e — Boundary compatibility.** The existing JSON media-type behavior,
other boundary rejection precedence, and problem shapes MUST remain unchanged.

**SI-02a — Principal scope.** The operation key MUST be
`(tenant_id, subject_id, submission_id)` from validated trust and parsed UUID.
The same UUID in another tenant or subject MUST have independent meaning.

**SI-02b — Exact original request.** Equality MUST compare the originally
requested optional Thread selector and
the exact UTF-8 bytes of the original sequence-1 `user_message`; no trimming,
Unicode normalization, effective correction projection, or digest-only match
is permitted. Omitted and null `thread_id` retain their existing equivalence;
the new-Thread selector is different from an explicit UUID, even the UUID
allocated by the original acceptance.

**SI-02c — Representation equivalence.** JSON whitespace/escaping/member order,
UUID text case, and synchronous versus streaming delivery are not semantic
input.

**SI-02d — Owned-key drift.** Changed semantic input under an owned existing
key MUST return
`409 submission-identity-conflict`, with no receipt, canonical mutation, or
provider/Tool invocation.

**SI-02e — Explicit Thread rejection.** A fresh explicit unknown or non-owned Thread MUST
return indistinguishable `404 not-found` without claiming another owner's key.

**SI-02f — Client selector guidance.** The contract copy MUST tell clients to
preserve the originally requested
selector: when the original request omitted/null-selected a Thread, keep that
selector on retries even after `turn.started` reports an allocated `thread_id`.
Adding that explicit UUID to the retry is semantic drift and returns 409.

**SI-03a — Atomic acceptance.** An identified fresh request MUST bind its key,
original selector, accepted Thread/Turn, and a server-private creator-operation
UUID in one transaction with the initial Turn, sequence-1 input, and
generation-1 lease; a newly allocated Thread belongs to that transaction too.
The binding key MUST be unique, and a Turn MUST have at most one such binding.
A database uniqueness constraint independently enforces the key. A proven
rollback leaves no binding or
partial new Thread, Turn, input, or lease. A new explicit Thread must already
exist and remain owned at final acceptance. No identity binding is written for
an unidentified request, which retains the existing acceptance path.

**SI-03b — Lock namespace, key, and ordering.** Acceptance and reconciliation
MUST acquire the submission lock with the two-`int4` form of
`pg_advisory_xact_lock`, whose key space is separate from the existing
single-`bigint` Item and startup-migration locks. Compute the SHA-256 digest
over the fixed ASCII tag `koduck.chat.submission.v1`, followed by tenant then
subject UTF-8 values (each prefixed by its unsigned 64-bit big-endian byte
length), then the 16 parsed submission UUID bytes. Derive the two signed
32-bit big-endian keys from the first eight bytes of that digest; reuse the
existing `sha2` dependency.
The encoding MUST include all three scope values, be unambiguous and stable
across processes, and treat UUID text case identically. Hash collisions may
delay another submission but MUST NOT change equality or authorization.
Within each transaction, acquire submission first, then the existing Item
lock if canonical acceptance/reconciliation uses it; no path may acquire them
in reverse order. Recheck the full owned key under the submission lock before
allocating canonical state. Reuse canonical write logic within this same
transaction, not a helper that opens or commits a separate transaction.
PostgreSQL documents the separation of these two key spaces in its
[advisory-lock functions](https://www.postgresql.org/docs/current/functions-admin.html#FUNCTIONS-ADVISORY-LOCKS).

**SI-04 — Ownership outcomes.** The acceptance port MUST distinguish a
`Created` outcome containing the original lease-bearing accepted Turn from
an `Existing` receipt containing only public submission/Thread/Turn identity.
Only the invocation whose private creator-operation UUID committed that
binding may receive `Created`, including reconciliation within that same
invocation. A different invocation MUST receive only `Existing`; it MUST NOT
renew a lease, call the provider or Tool boundary, emit a started event, append
Items, interrupt the original Turn, or reconstruct a dispatch token. The
receipt carries no generation, approval, credentials, or input. Process
restart MUST NOT turn an existing key into a new execution grant; an orphan
remains governed by existing fenced cancellation, without automatic takeover.
Carry observation as an owned successful result variant through the runner
and `TurnService`, rather than fabricating a completed `TurnResult` or
classifying the receipt as a provider failure. Only the created branch may
enter existing liveness/execution/stream-publication code.

**SI-05 — Retry wire outcome.** For an exact accepted-key retry on either chat
route, before any SSE header or event, return `202 application/json` with
exactly `submission_id`, `thread_id`, `turn_id`, and `status: accepted`. UUIDs
use canonical lowercase hyphenated text. `accepted` describes the immutable
acceptance outcome, not current Turn lifecycle or inference success. The same
receipt MUST be returned for started, recovery-pending, completed, failed,
interrupted, and cancelled Turns while their canonical data remains present.
Do not return Items/usage, wait for the original producer, subscribe to its
stream, or send SSE on that retry. A first `Created` invocation retains the
existing synchronous/SSE lifecycle response and cancellation behavior.
Conflicts and unavailable outcomes use the existing exact problem body, with
`409 submission-identity-conflict` and `503 durability-unavailable`
respectively; they expose no accepted identities.

**SI-06a — Stable invocation evidence.** Allocate the private creator-operation
UUID once per fresh acceptance invocation and retain its proposed Thread,
Turn, and input identities through settlement.

**SI-06b — Bounded read-only settlement.** Make one acceptance write attempt.
After an unavailable write or timeout, perform at most one read-only
reconciliation under the same scoped key lock and SI-03b lock ordering. Never
make a second acceptance write inside settlement or infer success from a
timed-out commit.

**SI-06c — Proof of a live creator.** Return `Created` only if the complete
committed binding matches
this creator and original input, and the Turn is still started with the
expected unfenced, unexpired generation-1 lease.

**SI-06d — Observed or expired creator.** A complete matching binding
created by another invocation, or this creator's now terminal/fenced/expired
Turn, returns `Existing` without authority. A later request uses its own
invocation identity and can only observe a prior committed key.

**SI-06e — Unproven or conflicting evidence.** Proven drift returns the typed
conflict; missing, corrupt, or unprovable state returns unavailable, with no
identity publication or provider call.

**SI-07a — Lookup and preparation order.** Validate transport/input and trust
before a bounded owned key lookup. The preliminary lookup MUST be read-only,
without an advisory or row lock; it observes committed state and does not
reserve absence. Resolve existing/conflicting keys before fresh prior-history
preparation. An unavailable preliminary lookup returns
`503 durability-unavailable` without prior-history preparation or an acceptance
write. On no binding, prepare CAND-13 history unchanged, then atomically
recheck/accept under SI-03a/SI-03b.

**SI-07b — Preparation rejection race.** If preparation rejects, perform one
further bounded unlocked key lookup: a concurrently committed exact key returns
its receipt, a drifted key conflicts, no key preserves the original rejection,
and unavailable lookup fails closed. This branch ends without an acceptance
write; it does not then proceed through the write/settlement branch.

**SI-07c — Per-attempt and reserved settlement budgets.** Preliminary/final
lookups and history reads use at most two seconds, including pool/lock waits,
clamped to remaining acceptance time. A permitted acceptance write MUST receive
its full two-second attempt budget and, if unavailable, one full two-second
read-only reconciliation budget. Do not compress either attempt by the
remaining acceptance clock or put them inside an outer timeout that truncates
settlement. One write plus at most one reconciliation has four seconds of
reserved deadline budget; no additional write, proof attempt or renewed budget
is allowed.

**SI-07d — Acceptance deadline and proven outcome handoff.** Identified requests
each have a monotonic ten-second acceptance deadline starting at fully validated
input/trust entry immediately before preliminary lookup. Count post-validation
scheduling, bounded preparation/decoding and prior database waits. Immediately
before starting the acceptance write, check cancellation first, then time.
If cancellation is observed, SI-07g's cancellation result takes precedence,
even with less than four seconds remaining. Otherwise, if less
than four seconds remain, MUST start no write and return unavailable without
accepted identities/provider work. Exactly four seconds is sufficient. Reserve
the full SI-07c write/proof budgets once the write starts; later expiry of the
acceptance deadline cannot truncate that settlement.

Once `Created` is proven, MUST hand it to the existing execution or durable
cancellation flow even if the ten-second acceptance deadline has passed. Do not
replace that known outcome with unavailable or abandon its live lease solely
because of this clock, and do not invent cancellation from budget expiry.
Existing liveness, fencing, actual cancellation and post-acceptance durability
failures remain authoritative. If settlement cannot prove acceptance after
its reserved attempts, return unavailable without IDs; any committed orphan
retains existing fenced recovery. The acceptance deadline controls whether this
fresh acceptance write may start, not whether proven accepted work is serviced.
It is not a hard end-to-end response cutoff for scheduler delay, provider
inference, SSE delivery,
or post-acceptance cancellation/recovery.

| Acceptance path | Maximum database deadline sum | Write-start gate and handoff |
| --- | --- | --- |
| Existing/conflicting key | Preliminary lookup: at most 2 s | No fresh write start; map the proven receipt/conflict without turning it into unavailable solely for elapsed acceptance time |
| Fresh key, new Thread | Preliminary lookup 2 s + write 2 s + reconciliation 2 s = 6 s | No history read; a permitted write MUST start no later than elapsed 6 s with at least 4 s remaining; proven Created is always handed off |
| Fresh key, explicit owned Thread | Preliminary lookup 2 s + history read 2 s + write 2 s + reconciliation 2 s = 8 s | At most 2 s of other pre-write work when both reads consume full budgets; a permitted write MUST start no later than elapsed 6 s with at least 4 s remaining and full settlement reservation |
| Preparation rejection | Preliminary lookup 2 s + history read 2 s + final lookup 2 s = 6 s | No write/reconciliation; final lookup is clamped to remaining acceptance time and returns its observed result or unavailable |
| Fresh path with less than 4 s remaining before write | Prior stages only; 0 acceptance writes/reconciliations | Observed cancellation returns the SI-07g result first; otherwise unavailable, both without accepted identities |

The post-rejection lookup and write/settlement are alternative paths, not
additive attempts. Nominal permitted proof work fits the ten-second budget by
reservation, but late scheduling after a known commit does not disown Created.
AC-6 MUST verify gate boundaries, complete paths, full settlement reservation,
and delayed proven-Created handoff; an isolated settlement timer is insufficient.

**SI-07e — Lock and work lifetime.** No lock may span provider execution. No
retry worker or queue is introduced.

**SI-07f — Retry disconnect isolation.** A disconnected retry MUST NOT cancel
or mutate the original owner.

**SI-07g — Cancellation before write.** A fresh identified creator whose
cancellation is observed before its acceptance write begins MUST start no
write or provider work and return the owned pre-acceptance cancellation result,
mapped to `409 turn-cancelled` without accepted identities if a response can
still be delivered. This cancellation wins over simultaneous insufficient
acceptance time; time shortage alone returns unavailable. The unidentified path
retains its current behavior.

**SI-07h — Cancellation racing acceptance.** If cancellation races the write,
settle it under SI-06a through SI-06e: a proven Created outcome enters the
existing durable cancellation path instead of provider execution; an unproved
commit remains unavailable and any committed orphan is closed by existing
fenced recovery when storage is available. A proven accepted Turn retains its
SI-08 binding and exactly one canonical terminal; observation never deletes it.

**SI-07i — Monotonic acceptance deadline boundary.** C-6 bounded atomic
acceptance MUST use a submission-specific clock dependency; the runner's
limited supporting wiring derives one request budget from its start reading.
Readings MUST be immediate, nonblocking and nondecreasing in that clock's
domain; remaining time is derived with checked, bounded arithmetic. A reading
earlier than its request start MUST fail closed before fresh acceptance, without
resetting or extending the budget. Do not use wall time, the
C-5 Tool deadline or the current Turn lease as the acceptance clock. The
production clock measures real monotonic elapsed time, including synchronous
preparation and `block_on` waits. Test clock substitution controls acceptance
readings only; it MUST NOT alter the real write/proof timeout, lease clock or
fenced-recovery timer. Runner cloning and Tool composition retain the clock
dependency, while each request owns its separate start/budget value.

**SI-08a — Migration compatibility.** A new additive idempotent migration MUST
create `chat_submissions` and register it in existing startup ordering; retain
all old rows and constraints. Repeated application is idempotent. No existing
Turn receives a guessed identity.

**SI-08b — Binding constraints.** The new relation MUST have the scoped primary
key, accepted-Turn foreign key, unique accepted-Turn constraint, original optional
selector and creator-operation identity. Enforce non-nil submission/creator UUIDs
and equality of any original explicit selector with the accepted Thread.
Foreign-key integrity prevents a binding outliving its canonical Turn.

**SI-08c — Lookup integrity.** Every binding lookup MUST validate the joined Thread's
tenant/subject ownership, referenced Turn, and original sequence-1 user input;
incomplete or inconsistent structure fails unavailable, never authorizes
execution. Distinguish an absent key from a binding whose joined canonical
data is missing; an inner join that hides a broken binding cannot establish
absence. Invalid structure cannot be treated as a fresh key or authorize work.

**SI-08d — Lifetime and deletion boundary.** Retain the binding while that
canonical Turn exists, including
after completion, correction, expiry, and process restart. No TTL, independent
binding deletion, or cleanup task is added. Any future deletion design must define
binding disposal together with canonical disposal before exposing deletion.

**SI-09 — Resource and disclosure bounds.** Read at most one binding and its
single canonical input per lookup; do not load complete history or outputs
for a receipt. Compare at most 65,536 input bytes; oversized/corrupt stored
input fails unavailable. Store no second copy of input or inference output
in the binding. Receipt and conflict diagnostics MUST contain none of the
input, other-owner metadata, creator-operation identity, lease/approval state,
database text, secrets, or private endpoints. Preserve current body, channel,
background-worker, output, and provider-context limits. No growing process
cache of all submission identities is permitted.

**SI-10 — Existing behavior preservation.** Requests without identity and new
distinct identities MUST retain CAND-1 lifecycle, subject-owned Resume-as-new-
Turn, CAND-2 default-deny execution/approval/interrupt, CAND-3 raw replay,
CAND-11 correction admission, and CAND-12/13 effective context and Tool
causality. Exact retry performs no new mutation, including after corrections
or larger history would make a fresh inference inadmissible. This contract
does not serialize different keys on one Thread or change deployment policy.

### Acceptance Flow [Optional]

The diagram summarizes the proposed SI-07 ordering; clause text and the
per-clause acceptance mapping remain authoritative. The rejected-preparation
branch ends at its final lookup, while successful preparation proceeds to
atomic acceptance. The ten-second acceptance deadline gates starting a write;
once the write starts, settlement has its full four-second reservation and proven
Created is handed off regardless of later acceptance-deadline expiry.

```mermaid
sequenceDiagram
  participant Client
  participant HTTP as HTTP boundary
  participant Runner
  participant Store as PostgreSQL
  Client->>HTTP: Identified chat or stream
  HTTP->>Runner: Validated input and trust
  Runner->>Store: Unlocked owned lookup (at most 2 s)
  Store-->>Runner: Existing, conflict, unavailable, or absent
  alt Existing, conflict, or unavailable
    Runner-->>HTTP: Receipt or typed rejection
  else Absent
    Runner->>Store: Existing bounded history read if needed (at most 2 s)
    Store-->>Runner: Canonical history or rejection
    Runner->>Runner: Prepare context and check acceptance clock
    alt Preparation rejected
      Runner->>Store: Final unlocked lookup (at most 2 s)
      Store-->>Runner: Exact, drift, absent, or unavailable
      Runner-->>HTTP: Receipt, conflict, original rejection, or unavailable
    else Preparation succeeded
      Runner->>Runner: Check cancellation first, then reserve 4 s before write
      alt Cancelled or less than 4 s remains
        Runner-->>HTTP: Rejection without acceptance write
      else Write permitted
        Runner->>Store: Submission lock, full recheck, then Item lock and atomic write (full 2 s budget)
        Store-->>Runner: Created, Existing, conflict, or uncertain acknowledgement
        opt Uncertain acknowledgement
          Runner->>Store: New read-only transaction, same lock order, reconcile once (full 2 s budget)
          Store-->>Runner: Proven Created, Existing, conflict, or unavailable
        end
        Runner->>Runner: Proven Created follows execution or durable cancellation even if acceptance clock elapsed
        Runner-->>HTTP: Typed outcome
      end
    end
  end
  HTTP-->>Client: Existing is 202 JSON; Created retains existing sync or SSE lifecycle
```

## Implementation Plan [Required]

**Complete task outcome**: The existing production chat path atomically binds
an authenticated optional submission identity to one Turn; exact repeated
submissions return its acceptance receipt, drift conflicts, and ambiguous
acknowledgement never creates another Turn or grants retry execution authority.

**Primary implementation boundary**: C-6 submission identity and atomic initial
Turn acceptance, including its bounded acceptance deadline. Request/result
types, HTTP mapping, runner branching/clock wiring, and
startup migration registration are limited supporting interfaces for that
single persistence invariant; no independently deliverable CAND-17 Thread
mutation-admission or provider policy is included. Deliver through one task implementation PR to
`dev` on this task branch after acceptance.

Allowed subtask statuses: `Not Started`, `In Progress`, `Blocked`, `Complete`,
or `N/A — <specific reason>`.

| ID | Objective or deliverable | Included scope | Status | Actual implementation evidence |
| --- | --- | --- | --- | --- |
| T-1 | Deliver the durable submission acceptance contract end to end | Owned identity/result/monotonic acceptance types, lookup and transaction, migration, bounded settlement, minimal runner/HTTP/startup clock wiring, authoritative contract copy and governed-file markers | In Progress | Core delivered on this branch: `domain/submission.rs`, `application/submission.rs` + `submission/acceptance.rs`, `ports.rs` identified-acceptance port with fail-closed defaults, `adapters/history/postgres/sqlx_executor/submission_child.rs`, migration `0010_cand_18_chat_submissions.sql`, runner `execute_submission_with_observer_and_cancellation` with `with_acceptance_clock`, wire three-state parsing and the 202 receipt, runtime registration and explicit system clock, contract copy `koduck-ai/docs/contracts/cand-18-chat-submission-v1.md`, governed-file markers. Outstanding: none identified for the core contract; preservation/integrity case completion continues under T-2 |
| T-2 | Prove the same boundary's acceptance and preservation invariants | AC-0 deferred-COMMIT driver preflight, test-first semantic and migrated PostgreSQL/Axum cases, necessary harness/double changes, primary integration and supplementary lib fault evidence, actual LCOV/gate and revision-bound delivery results | In Progress | Executed against the disposable migrated PostgreSQL 18 container and the real Axum router: AC-0's three probe cases Pass; the four-contender race, atomic acceptance/retry/drift/scope, the production-runner identified flows (created execution, exact-retry observation, typed conflict, SI-07g pre-acceptance cancellation, SI-07h racing durable cancellation, SI-07d remaining-4-second gate with exactly-4 s admission, SI-07i fail-closed earlier reading), the AC-5 isolated deferred-trigger reconciliation fixture (delayed commit proven Created; abort leaves zero rows), SI-07b rejected preparation, the SI-01d default-port fail-closed behavior, migration idempotence, the tool-denial/stream-ended preservation cases, and the real-`run` assembly through the explicit acceptance-clock composition. Remaining: AC-3 live pg_locks namespace evidence, AC-4 lifecycle/foreign-Thread/representation remainder, AC-5 creator-mismatch/fenced/expired proof branches, AC-6 read-clamp evidence, AC-7 initial-unavailable 503, AC-8 constraint-attempt cases, AC-9 LCOV/CI/exact-revision review |

After this ADR is Accepted, T-2 first performs AC-0's driver/fixture preflight
using a test-only relation in an isolated disposable database. Then its first
failing feature behavior test precedes T-1 production changes; remaining cases
run alongside T-1. Verification is part of the same slice, not a second
implementation outcome.

**Affected paths**:

- `koduck-ai/src/domain/` and `koduck-ai/src/application/submission.rs` (new
  focused identity/acceptance values as needed), `application/mod.rs`,
  `application/ports.rs`, and `application/runner.rs` with focused children.
- `koduck-ai/src/application/submission/acceptance.rs` (new): consumer-owned
  monotonic clock and budget values; runner constructor/composition and
  `koduck-ai/src/runtime/mod.rs` (`koduck_ai::runtime::run`) for explicit
  production clock assembly.
- `koduck-ai/src/adapters/history/postgres.rs`,
  `adapters/history/postgres/sqlx_executor.rs`, and a focused
  `adapters/history/postgres/sqlx_executor/submission.rs`; reuse
  `adapters/history/postgres/commit_reconciliation.rs` where semantics match.
- `koduck-ai/src/adapters/http/mod.rs`, `adapters/http/wire.rs` or focused
  submission children, and `koduck-ai/src/runtime/mod.rs` for migration
  registration and only necessary pre-stream response plumbing.
- `koduck-ai/migrations/0010_cand_18_chat_submissions.sql` (new),
  `koduck-ai/docs/contracts/cand-18-chat-submission-v1.md` (new), and the
  existing `koduck-ai/docs/contracts/cand-1-rest-sse-v1.md` implementation copy.
- Focused children of the existing coverage-selected
  `koduck-ai/tests/cand_12_projection.rs` and `koduck-ai/tests/postgres_cand_11.rs`
  targets for submission and real Axum tests; their migration fixtures, other
  existing `koduck-ai/tests/` doubles only when required by changed port types,
  and colocated adapter unit tests for deterministic commit-loss injection.
- A focused `koduck-ai/tests/postgres_cand_11/submission_fixture.rs` (new)
  beside the existing harness: private migrated schema/search_path, production
  pool-option matching, test-only connection occupancy and backend-PID evidence;
  manual acceptance clock fixtures beneath the selected cand_12_projection
  submission test modules.
- This ADR, `docs/adr/INDEX.md`, and the selected row/change log in the source
  ADD. No change to the ADD's approved candidate outcome or design is proposed.

### Stable Implementation Touchpoints [Conditionally Required — source or configuration implementation]

Existing symbols below represent source
`a0f81b54ca13eb8133c364caf1edb3dfa1c6c96d`. Rows explicitly marked Proposed
describe new injection/fixture anchors, not symbols present at that revision.

| Path | Stable symbol or contract anchor | Key code excerpt, when needed | Purpose | Source revision |
| --- | --- | --- | --- | --- |
| `koduck-ai/src/application/ports.rs` | `koduck_ai::application::ports::TurnCommand`, `TurnHistory::accept_initial`, `AcceptedTurn` | N/A — stable symbols suffice | Carry optional identity and distinguish observation from the accepted lease-bearing owner | `a0f81b54ca13eb8133c364caf1edb3dfa1c6c96d` |
| `koduck-ai/src/application/runner.rs` | `koduck_ai::application::runner::TurnRunner::execute_with_observer_and_cancellation` | N/A — stable symbol suffices | Lookup before fresh preparation and branch again on final transaction outcome before liveness, started, or provider work | `a0f81b54ca13eb8133c364caf1edb3dfa1c6c96d` |
| `koduck-ai/src/application/submission/acceptance.rs` (new); `application/runner.rs` | Proposed `koduck_ai::application::AcceptanceClock`, `AcceptanceInstant`, `AcceptanceBudget` and `TurnRunner::with_acceptance_clock`; existing `koduck_ai::application::TurnRunner::new` and `TurnRunner::with_tool_executor` | N/A — proposed dependency described below | Own monotonic policy at its consumer, inject an owned clock, retain it through clone/Tool composition; derive an independent start/budget per identified request | Proposed — new clock anchors absent at the inspected source revision |
| `koduck-ai/src/adapters/history/postgres/sqlx_executor.rs` | `koduck_ai::adapters::history::postgres::sqlx_executor::SqlxPostgresExecutor::accept_initial`, `accept_initial_with_identity_async` | N/A — stable symbols suffice | Existing initial transaction and private generated identities; route identified acceptance to a focused child | `a0f81b54ca13eb8133c364caf1edb3dfa1c6c96d` |
| `koduck-ai/src/adapters/history/postgres/commit_reconciliation.rs`; `koduck-ai/src/adapters/history/postgres.rs` | `commit_reconciliation::accepted_turn`, `lock_operation`; `postgres::settle_commit_attempt` under `koduck_ai::adapters::history` | Existing `lock_operation` uses `pg_advisory_xact_lock($1)` with a bigint from Item UUID bytes | Reuse bounded settlement and canonical logic; identified paths add SI-03b's separate submission lock before any Item lock, within one transaction | `a0f81b54ca13eb8133c364caf1edb3dfa1c6c96d` |
| `koduck-ai/src/adapters/http/wire.rs`; `koduck-ai/src/adapters/http/mod.rs` | `koduck_ai::adapters::http::wire::parse_turn_request`, `TurnRequestDocument`; `koduck_ai::adapters::http::TurnService`, `HttpAdapter::handle_stream_controlled` | Existing `TurnRequestDocument::thread_id` is `Option<String>` | Add submission parsing that distinguishes absent, null and value, without changing thread_id's null/omission equivalence; retain private typed serialization, successful receipt propagation and pre-SSE negotiation | `a0f81b54ca13eb8133c364caf1edb3dfa1c6c96d` |
| `koduck-ai/src/runtime/mod.rs` | `koduck_ai::runtime::run`, `koduck_ai::runtime::apply_startup_migrations`, `koduck_ai::runtime::handle_stream_request`, `koduck_ai::runtime::build_router` | run uses `PgPoolOptions::new()` and `TurnRunner::new(...).with_tool_executor(...)` | Explicitly compose the system acceptance clock; register migration and exercise pre-SSE decisions; preserve production pool settings and use this assembly as AC-0's option baseline | `a0f81b54ca13eb8133c364caf1edb3dfa1c6c96d` |
| `koduck-ai/migrations/0001_cand_1_history.sql` | `threads`, `turns`, `turn_items`, `turn_leases` | N/A — stable schema anchors suffice | Preserve canonical tables and ownership/terminal constraints; add a referencing relation in a new migration | `a0f81b54ca13eb8133c364caf1edb3dfa1c6c96d` |
| `koduck-ai/tests/postgres_cand_11/harness.rs`; `tools/sonarqube/rust-coverage.sh` | `harness::Harness`; coverage command's `--test postgres_cand_11` and `--test cand_12_projection` anchors | N/A — stable symbols/command anchors suffice | Reuse migrated disposable database and selected integration binaries; the coverage script is inspection-only and unchanged | `a0f81b54ca13eb8133c364caf1edb3dfa1c6c96d` |
| `koduck-ai/tests/postgres_cand_11/harness.rs`; `tests/postgres_cand_11/submission_fixture.rs` (new) | Existing `harness::CorruptFixture::create` and `CorruptFixture::teardown`; Proposed SubmissionFixture | CorruptFixture sets PgConnectOptions search_path and closes its dedicated pool before dropping its schema | Reuse the schema/pool lifecycle pattern, but create all canonical relations in the private schema, omit public fallback, match production pool options and collect writer/proof PIDs | Existing CorruptFixture at `a0f81b54ca13eb8133c364caf1edb3dfa1c6c96d`; SubmissionFixture is Proposed |

### Monotonic Acceptance Clock Design [Required]

SI-07i is the authoritative clock contract; SI-07c/SI-07d own the attempt
budgets and write-start gate. This section specifies composition and fixture
mechanics that implement those clauses.

The proposed `application/submission/acceptance.rs` owns `AcceptanceClock`,
`AcceptanceInstant` and `AcceptanceBudget`. The clock is a Send/Sync consuming
port returning an owned elapsed `Duration` value in the chosen clock's origin
domain, wrapped as AcceptanceInstant; the budget stores one request's start.
The system implementation owns a `std::time::Instant` origin. The coalescer's
existing explicit-Instant pattern informs value arithmetic; a dedicated
consuming port supplies the runner dependency.

Compose an owned `Arc<dyn AcceptanceClock>` into `TurnRunner` through a documented
`with_acceptance_clock` builder and explicitly supply the system implementation
from `runtime::run`. Existing `new(provider, history)` call shape retains a
system-clock default. Re-export only the consuming types/builder needed for
composition and integration tests; SI-07i governs clone/Tool retention and reads.
No global mutable clock, HTTP clock override, database-failure hook or test-only
production feature is added.

The test-only manual clock holds one origin and a synchronized offset under
SI-07i's reading contract. A staged history fixture advances it before returning
from lookup or preparation, and a test wrapper around the real acceptance port can advance it
after obtaining Created and before returning that result to the runner. These
fixture hooks supply AC-6's exact gate and late-Created inputs without sleeps
or Tokio's paused time. A separate invalid-clock fixture supplies a reading
earlier than request start to verify SI-07i's fail-closed result. AC-6 defines
the distinct policy assertions and real timeout/lock integration evidence.

**Migration and rollback strategy [Conditionally Required — this replaces or
changes existing behavior]**: T-1 adds and registers migration 0010; apply it
twice against populated disposable canonical data and verify no duplicate
relation/index/binding or changed existing rows. Historical Turns and legacy
requests need no backfill. Stop rollout preparation if atomicity, key drift,
authority separation, production framework behavior, or any required check
fails; no runtime deployment is authorized here. A later accepted operational
rollback may restore the previous binary while retaining the additive table
and canonical rows; that binary rejects `submission_id` as an unknown field,
so identified clients must not silently remove the identity and resend.
Disabling the identified path must fail closed rather than process it as an
unidentified new submission. Retain binding data for forward restoration; no
down migration or deduplication guarantee after downgrade is claimed. Actual
deployment, rollback execution, and any retention/deletion change require
their own applicable authorization.

### Engineering Exceptions [Conditionally Required — an engineering rule is exceeded or waived]

N/A — none is proposed. Focused submission ownership, limited parent wiring,
and revision-bound decomposition review must satisfy the binding engineering
limits. A discovered exception requires an ADR update and reapproval before
the exceptional change; AC-9 records final measurements and dispositions.

### Key State And Invariant Matrix [Required]

| State or precondition | Action or ordering | Expected observable outcome | Invariant owner and affected entry points | Check or explicit gap |
| --- | --- | --- | --- | --- |
| Valid key absent; new or owned Thread | Concurrent exact submissions enter final transaction | One creator; other calls observe that same identity | C-6: both chat routes, runner and direct acceptance port; one committed binding/Turn/input/lease per key | AC-2, AC-3 |
| Key present; any lifecycle, later correction or oversized context | Retry, including cross-route delivery | Constant 202 receipt; no new inference or mutation | C-6 identity plus C-2 branch; observation never obtains execution authority and compares original bytes | AC-4, AC-7 |
| Key present; changed content or requested selector | Repeat under same owned key | 409 conflict; zero mutation | C-6 exact-comparison invariant at lookup and transaction recheck | AC-4 |
| Preliminary lookup unavailable; fresh prior context would reject | Resolve the initial lookup before preparation | 503 durability-unavailable; no prior-history preparation or acceptance write | C-6 observation and C-2 stage ordering on both chat routes; unproven lookup cannot authorize preparation or acceptance | AC-7 |
| Same UUID under different owner; explicit foreign/missing Thread | Lookup or fresh acceptance | Independent scope or indistinguishable 404; no foreign receipt | C-6 ownership predicates and C-1 trust validation on both routes | AC-1, AC-4, AC-8 |
| Commit succeeds but acknowledgement is lost | Original creator reconciles; another request retries; process restarts | Proven same-invocation live creator may proceed once; every other observer receives receipt; unresolved outcome unavailable | C-6 settlement and C-2 execution branch; no second acceptance write or recreated lease authority | AC-5 |
| Lock blocked, transaction rolled back, or lookup unavailable | Deadline or disconnect occurs before/after commit | Bounded unavailable or existing proof; no partial proven-rollback rows; original orphan settles under existing recovery | C-6 attempts and C-1/C-2 cancellation; retry never cancels original owner | AC-5, AC-6 |
| Fresh identified creator; cancellation already observed, possibly less than 4 s remains | Check cancellation first before checking the acceptance deadline and acceptance write | 409 turn-cancelled where deliverable, including simultaneous time shortage; no binding or Turn | C-2 cancellation branch and C-6 entry; pre-acceptance rejection publishes no accepted identity and starts no inference | AC-6 |
| Initial lookup absent; context rejects while another caller commits | Final bounded lookup precedes rejection response | Receipt/conflict if newly bound; otherwise original rejection | C-2 preparation ordering and C-6 binding; fresh rejection cannot bypass proof of a now-known accepted key | AC-7 |
| Mapping corrupt, migration repeats, or canonical lifecycle changes | Read binding, migrate, expire, correct, or interrupt | Corruption unavailable; migration idempotent; binding unchanged across existing transitions | C-6 structure and canonical ownership; no isolated deletion or TTL | AC-8 |
| Preliminary lookup while another transaction holds submission lock | Read committed state without taking a lock; final acceptance then waits in submission namespace | Initial observation/preparation is not reserved or serialized; final recheck is authoritative | C-6 lock ownership and C-2 stage ordering; submission precedes Item lock and never shares the bigint namespace | AC-3, AC-6, AC-7 |
| Identified request traverses the complete acceptance path | Consume lookup/context budgets, reserve settlement before write, or perform alternative rejection lookup | Fresh owned-Thread database budget at most 8 s; no write with less than 4 s remaining from 10 s acceptance deadline; permitted write/proof get full budgets | C-6 acceptance deadline/reserved settlement; limited C-2 runner wiring at both routes/direct identified acceptance; rejection before write cannot create an orphan | AC-6 |
| Created proven, then acceptance clock crosses 10 s | Handoff to existing lifecycle with or without actual cancellation | Normal owner executes; cancelled owner durably cancels; neither is disowned solely for elapsed acceptance time | C-2 Created handoff and existing liveness/cancellation; known acceptance cannot become unavailable because of acceptance-deadline expiry | AC-6, AC-7 |
| Runner cloned or Tool boundary composed with an injected clock | Start separate identified requests, advance test policy time while real database timers run | Shared chosen clock, independent request budgets; exact threshold decisions do not alter real settlement or lease timers | C-6 bounded acceptance values with limited C-2 runner/runtime clock wiring; no transport-controlled or global time source | AC-6 |
| Commit future dropped; pool may return the writer session or another session | Match production pool options, control idle-session availability in fixture, record writer/proof PID | Both realizable paths prove commit within the same full 2 s proof budget, including pending COMMIT/ROLLBACK cleanup when PID is reused | C-6 SQLx/pool boundary; connection reuse cannot invent rollback or truncate proof | AC-0, AC-5 |

Cancellation, deadlines, input boundaries, and cross-component consistency
apply. Multi-instance execution takeover and new deletion endpoints are
outside this candidate; no verification of those new capabilities is claimed.
The database tests still use independent handles/connections to prove the
unique acceptance invariant. Deployment/load capacity is unmeasured and is
not an acceptance claim.

## Contract-To-Check Traceability [Conditionally Required — source or configuration implementation]

The authoritative location for each SI clause is this ADR's
`Submission Contract` subsection at
`docs/adr/ADR-0018-chat-submission-identity-and-atomic-acceptance.md`.

| Clause ID | Authoritative contract path and heading | Exact normative requirement | Acceptance check or deterministic test IDs | Explicit coverage method |
| --- | --- | --- | --- | --- |
| SI-01 | This ADR — Submission Contract, SI-01a through SI-01e | Optional strict non-nil hyphenated UUID distinguishing absent/null/value (SI-01a); strict body members/JSON and 1..65,536 UTF-8 input bytes (SI-01b); authenticated identity before any lookup or effect (SI-01c); direct acceptance-port guards that cannot bypass input/identity validation and never silently discard a supplied key (SI-01d); unchanged existing media type, rejection precedence and problem shapes (SI-01e) | AC-1, AC-2, AC-9 | AC-1 drives SI-01a on both routes (omitted legacy success, present null 400, each valid/invalid UUID form, canonical parsed identity, no history operation on rejection), SI-01b (duplicate/unknown/malformed cases and 0/65,536/65,537 UTF-8 byte boundaries with exact 400 and zero writes), and SI-01c (missing/invalid trust returns exact 401 plus challenge with no lookup, history, provider or Tool work); AC-2 drives SI-01d (invalid public-field commands at the direct port cause no DB I/O; valid identified acceptance creates a binding through the production adapter); AC-1 and AC-9 re-check SI-01e (existing parsed boundary fixtures plus real Axum JSON media-type parameters and competing invalid-boundary cases) |
| SI-02 | This ADR — Submission Contract, SI-02a through SI-02f | Operation key is (tenant_id, subject_id, submission_id) with independent per-owner meaning (SI-02a); equality compares the originally requested optional Thread selector and the exact original sequence-1 user_message UTF-8 bytes with no trimming, normalization, projection or digest-only match (SI-02b); JSON whitespace/escaping/order, UUID text case and delivery route are not semantic input (SI-02c); owned-key drift returns exact 409 with no receipt, mutation or effects (SI-02d); fresh unknown/foreign explicit Thread returns indistinguishable 404 without claiming the key (SI-02e); the contract copy directs clients to preserve the originally requested selector (SI-02f) | AC-4 | AC-4 drives SI-02a (same UUID under a different tenant and then a different subject creates independent bindings; retries disclose only each owner's IDs), SI-02b (whitespace/Unicode-normalization drift, corrected effective input, None versus the explicit allocated UUID, and explicit-selector equality against the canonical sequence-1 input), SI-02c (equivalent JSON escaping/spacing/member order, uppercase UUID and cross-route retries return identical identity), SI-02d (changed original bytes or selector returns conflict with before/after binding, lease, canonical row and provider/Tool effect comparisons), SI-02e (valid new key with nonexistent versus other-subject/tenant Thread gives an identical problem shape and zero new binding/Turn), and SI-02f (real new-Thread request receives started/allocated IDs, exact absent/null retry succeeds, and adding the returned explicit Thread to the same key yields 409; semantic contract-copy review checks the guidance) |
| SI-03 | This ADR — Submission Contract, SI-03a and SI-03b | Unique atomic binding with canonical acceptance, initial lease and any new Thread in one transaction, no partial rows on proven rollback, and no binding for unidentified requests (SI-03a); separate two-int4 submission advisory-lock namespace keyed by the complete owner/key digest, submission-before-Item lock ordering, and same-transaction full-key recheck reusing canonical write logic (SI-03b) | AC-2, AC-3, AC-5, AC-6, AC-8 | AC-2 and AC-8 assert scoped rows, constraints and the existing no-identity path for SI-03a through the real migration, synchronized contention and induced transaction abort; AC-3, AC-5 and AC-6 drive SI-03b through hash-encoding fixtures for all three scope values with UUID-case equivalence, real pg_locks/held-lock tests proving namespace non-overlap and no Item lock while waiting on the submission lock, collision affecting waiting only, and the final full-key recheck inside the one canonical transaction |
| SI-04 | This ADR — Submission Contract, SI-04 | Only original creating invocation owns execution; observers change no lease, Items, effects, or cancellation state | AC-3, AC-5, AC-6, AC-7 | Count actual provider requests/Tool effects plus canonical rows/lease before and after retry; reconstruct independent runtime handles |
| SI-05 | This ADR — Submission Contract, SI-05 | Constant exact 202 JSON receipt for every lifecycle and both routes before stream emission; typed exact problems | AC-4, AC-7 | Real Axum response status/media type/parsed field equality and complete response bytes prove absence of SSE |
| SI-06 | This ADR — Submission Contract, SI-06a through SI-06e | One private creator-operation UUID per fresh invocation retained through settlement (SI-06a); one acceptance write plus at most one locked read-only reconciliation, never a second write or inferred success (SI-06b); Created only for a complete matching live-creator proof (SI-06c); another invocation or a terminal/fenced/expired creator observes without authority (SI-06d); proven drift conflicts and missing/corrupt/unprovable state returns unavailable with no identity publication or provider call (SI-06e) | AC-5, AC-6, AC-8 | AC-5 drives SI-06a (production write/lookup fault trace and durable rows prove the same proposed identities are reconciled), SI-06c (creator, input, status, generation, fencing and expiry varied individually; only the complete valid proof grants authority), SI-06d (independent caller, recreated runner and each dead-owner state return an identical receipt without lease mutation or effects), and SI-06e (each proof-failure fixture asserts the typed outcome, exact problem and no published identities or provider work); AC-5 and AC-6 drive SI-06b (lost/aborted acknowledgement cases count attempts and verify zero reconciliation mutation under SI-03b lock ordering); AC-8 re-checks SI-06e's corrupt-structure unavailable outcome |
| SI-07 | This ADR — Submission Contract, SI-07a through SI-07i | Unlocked preliminary lookup with 503 before preparation/write when unavailable, then locked atomic recheck (SI-07a); one alternative final unlocked lookup after rejected preparation (SI-07b); pre-write reads clamp to remaining acceptance time while a permitted write and reconciliation each retain their full 2 s within the 4 s reservation (SI-07c); monotonic 10 s acceptance deadline with a cancellation-first remaining-4-s write gate and proven-Created handoff independent of later expiry (SI-07d); no provider-spanning lock or retry worker/queue (SI-07e); retry disconnect isolation from the original owner (SI-07f); observed pre-write cancellation wins over simultaneous time shortage with no write or provider work (SI-07g); cancellation racing acceptance settles under SI-06 with durable cancellation or truthful unavailable (SI-07h); owned monotonic acceptance clock retained through composition with real database/lease timers unchanged (SI-07i) | AC-3, AC-5, AC-6, AC-7, AC-9 | AC-3, AC-7 and AC-9 drive SI-07a (held submission lock does not block the initial committed read; an existing key bypasses poisoned preparation; initial unavailable returns exact 503 with zero preparation and writes; a late binding is seen in the final transaction), SI-07b (concurrent exact/drift/absent/unavailable cases assert receipt/409/original rejection/503 with zero acceptance writes), SI-07e (retry completes while the original provider blocks; live locks release; structural runtime review finds no new retry queue or worker), and SI-07f (real Axum retry body drop leaves the original owner, lease and canonical rows unchanged); AC-5 and AC-6 drive SI-07h (before/after-COMMIT cancellation cases restore storage, assert no provider work after cancellation, one durable terminal, retained binding, and truthful unavailable when unproven); AC-6 drives SI-07c (staged monotonic timing and real pool/lock waits assert clamped read budgets and full write/proof budgets with no outer truncation), SI-07d (manual clock at 4 s minus 1 ns, exactly, and plus 1 ns; cancellation plus shortage yields 409 before any write; uncancelled shortage yields 503; advancing past 10 s after proven Created still executes or actually cancels), SI-07g (both routes and the direct identified runner assert 409 or undeliverable disconnect with zero binding, Turn and effects), and SI-07i (manual forward-only readings with exact arithmetic, an invalid earlier-than-start clock failing closed, clone/Tool-composed runners retaining the clock, and system-clock elapsed measurement across block_on with unchanged real timeout and lease checks); AC-9 records the structural no-queue/no-worker review |
| SI-08 | This ADR — Submission Contract, SI-08a through SI-08d | Additive registered idempotent migration 0010 preserving old rows without backfill (SI-08a); scoped primary key, accepted-Turn foreign key and uniqueness, original optional selector with explicit-selector equality, and non-nil submission/creator UUIDs (SI-08b); every lookup validates the joined Thread owner, Turn and original sequence-1 input, distinguishing absent keys from malformed bindings (SI-08c); binding retained for the canonical Turn's lifetime with no TTL, isolated deletion or cleanup (SI-08d) | AC-8 | AC-8 drives SI-08a (populated 0001..0009 fixture starts at 0010, applies the startup sequence twice, and compares legacy rows and bindings before/after), SI-08b (individually attempts duplicate scoped key, duplicate accepted Turn, orphan FK, mismatched explicit selector and nil submission/creator, asserting each DB constraint rejection), SI-08c (truly absent key versus bindings with missing/mismatched Thread/Turn, wrong owner, missing/wrong sequence-1 input, unsupported payload or oversized input; only genuine absence is eligible for fresh acceptance), and SI-08d (retry after terminal, correction, lease expiry and restart yields the same IDs; rolled-back canonical deletion preserves FK integrity; semantic schema/runtime review finds no TTL, delete job or independently authorized deletion) |
| SI-09 | This ADR — Submission Contract, SI-09 | One bounded input lookup, no copied input/output/cache, no sensitive diagnostic fields, existing bounds retained | AC-1, AC-4, AC-8, AC-9 | Maximum/corrupt input fixtures, bounded-query/schema inspection, parsed diagnostic fields, routed bounds regressions and structural review |
| SI-10 | This ADR — Submission Contract, SI-10 | Preserve lifecycle/Resume/tools/corrections/context/replay for legacy and fresh keys; retry creates no new work | AC-2, AC-6, AC-7, AC-9 | Existing routed regressions plus identified creator cases and before/after canonical replay comparison |
| WIRE-01 | `docs/adr/ADR-0001-provider-neutral-turn-kernel.md` — Detailed Design, authoritative v1 wire contract | Preserve existing successful fresh sync/SSE events, problem bodies, trust, interrupt, ownership and raw context guards except explicit SI-01/SI-05 extensions | AC-1, AC-2, AC-6, AC-7, AC-9 | Real fresh HTTP/SSE requests and existing CAND-1 contract fixtures, invalid-identity, interruption, subject-ownership and bounds tests |
| TC-01 | `docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md` — Normative Contract Clauses, TC-01 | Preserve single authority: every native Tool/MCP invocation enters C-5; no direct effect path exists | AC-6, AC-7, AC-9 | Execute that clause's existing mapping in ADR-0003's Contract-To-Check Traceability plus AC-9's routed preservation re-execution; AC-6/AC-7 prove a real runner Tool retry creates no second effect |
| TC-02 | `docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md` — Normative Contract Clauses, TC-02 | Preserve default denial: unknown or invalid metadata denies with zero approval and dispatch | AC-6, AC-7, AC-9 | Execute that clause's existing mapping in ADR-0003's Contract-To-Check Traceability plus AC-9's routed preservation re-execution; AC-6/AC-7 prove a real runner Tool retry creates no second effect |
| TC-03 | `docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md` — Normative Contract Clauses, TC-03 | Preserve Turn profile immutability against untrusted content or approval widening | AC-6, AC-7, AC-9 | Execute that clause's existing mapping in ADR-0003's Contract-To-Check Traceability plus AC-9's routed preservation re-execution; AC-6/AC-7 prove a real runner Tool retry creates no second effect |
| TC-04 | `docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md` — Normative Contract Clauses, TC-04 | Preserve exact approval: accepted D-6 authorizes exactly one fully matching D-7 | AC-6, AC-7, AC-9 | Execute that clause's existing mapping in ADR-0003's Contract-To-Check Traceability plus AC-9's routed preservation re-execution; AC-6/AC-7 prove a real runner Tool retry creates no second effect |
| TC-05 | `docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md` — Normative Contract Clauses, TC-05 | Preserve authenticated decisions: only a validated same-tenant scoped approver resolves D-6 without existence leakage | AC-6, AC-7, AC-9 | Execute that clause's existing mapping in ADR-0003's Contract-To-Check Traceability plus AC-9's routed preservation re-execution; AC-6/AC-7 prove a real runner Tool retry creates no second effect |
| TC-06 | `docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md` — Normative Contract Clauses, TC-06 | Preserve non-authoritative projections: D-3 is durable append-only projection and never authority | AC-6, AC-7, AC-9 | Execute that clause's existing mapping in ADR-0003's Contract-To-Check Traceability plus AC-9's routed preservation re-execution; AC-6/AC-7 prove a real runner Tool retry creates no second effect |
| TC-07 | `docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md` — Normative Contract Clauses, TC-07 | Preserve lease fencing: current lease is checked at prepare, dispatch and commit; fenced results do not reach the model | AC-6, AC-7, AC-9 | Execute that clause's existing mapping in ADR-0003's Contract-To-Check Traceability plus AC-9's routed preservation re-execution; AC-6/AC-7 prove a real runner Tool retry creates no second effect |
| TC-08 | `docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md` — Normative Contract Clauses, TC-08 | Preserve bounded retry: only one proven-pre-effect retry uses new D-7, fresh policy/approval and one additional Turn attempt slot | AC-6, AC-7, AC-9 | Execute that clause's existing mapping in ADR-0003's Contract-To-Check Traceability plus AC-9's routed preservation re-execution; AC-6/AC-7 prove a real runner Tool retry creates no second effect |
| TC-09 | `docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md` — Normative Contract Clauses, TC-09 | Preserve exact concurrency, attempt, time and output bounds | AC-6, AC-7, AC-9 | Execute that clause's existing mapping in ADR-0003's Contract-To-Check Traceability plus AC-9's routed preservation re-execution; AC-6/AC-7 prove a real runner Tool retry creates no second effect |
| TC-10 | `docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md` — Normative Contract Clauses, TC-10 | Preserve cancellation truth: cancellation closes pending work or produces a truthful bounded executor outcome | AC-6, AC-7, AC-9 | Execute that clause's existing mapping in ADR-0003's Contract-To-Check Traceability plus AC-9's routed preservation re-execution; AC-6/AC-7 prove a real runner Tool retry creates no second effect |
| TC-11 | `docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md` — Normative Contract Clauses, TC-11 | Preserve untrusted results: Tool/MCP output cannot grant authority and is delivered only after fenced durable commit | AC-6, AC-7, AC-9 | Execute that clause's existing mapping in ADR-0003's Contract-To-Check Traceability plus AC-9's routed preservation re-execution; AC-6/AC-7 prove a real runner Tool retry creates no second effect |
| TC-12 | `docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md` — Normative Contract Clauses, TC-12 | Preserve durable concurrency: conditional durable transitions permit one winner and no duplicate dispatch of the same D-7 | AC-6, AC-7, AC-9 | Execute that clause's existing mapping in ADR-0003's Contract-To-Check Traceability plus AC-9's routed preservation re-execution; AC-6/AC-7 prove a real runner Tool retry creates no second effect |
| TC-13 | `docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md` — Normative Contract Clauses, TC-13 | Preserve disabled recovery: unavailability with no legacy or direct fallback | AC-6, AC-7, AC-9 | Execute that clause's existing mapping in ADR-0003's Contract-To-Check Traceability plus AC-9's routed preservation re-execution; AC-6/AC-7 prove a real runner Tool retry creates no second effect |
| TC-14 | `docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md` — Normative Contract Clauses, TC-14 | Preserve minimized audit: each correlated audit record is at most 16,384 bytes and excludes credential values and raw action parameters/result content | AC-6, AC-7, AC-9 | Execute that clause's existing mapping in ADR-0003's Contract-To-Check Traceability plus AC-9's routed preservation re-execution; AC-6/AC-7 prove a real runner Tool retry creates no second effect |
| PLB-1 | `docs/adr/ADR-0005-provider-delta-coalescing-and-512-item-turn-budget.md` — Provider-Level Buffer Contract, PLB-1 | Preserve raw provider delta count not incrementing the Item budget until one coalesced Item is planned | AC-9 | Execute that clause's existing mapping in ADR-0005's traceability, including raw fragment/UTF-8/latency boundaries and coalescing-count assertions |
| PLB-2 | `docs/adr/ADR-0005-provider-delta-coalescing-and-512-item-turn-budget.md` — Provider-Level Buffer Contract, PLB-2 | Preserve the 16,384 UTF-8-byte or 500 ms flush with splits only at valid UTF-8 boundaries | AC-9 | Execute that clause's existing mapping in ADR-0005's traceability, including raw fragment/UTF-8/latency boundaries and coalescing-count assertions |
| PLB-3 | `docs/adr/ADR-0005-provider-delta-coalescing-and-512-item-turn-budget.md` — Provider-Level Buffer Contract, PLB-3 | Preserve flushing pending text before every semantic, Tool, cancellation, interruption or terminal boundary | AC-9 | Execute that clause's existing mapping in ADR-0005's traceability, including raw fragment/UTF-8/latency boundaries and coalescing-count assertions |
| PLB-4 | `docs/adr/ADR-0005-provider-delta-coalescing-and-512-item-turn-budget.md` — Provider-Level Buffer Contract, PLB-4 | Preserve publish-only-after-append with no uncommitted coalesced bytes published | AC-9 | Execute that clause's existing mapping in ADR-0005's traceability, including raw fragment/UTF-8/latency boundaries and coalescing-count assertions |
| PLB-5 | `docs/adr/ADR-0005-provider-delta-coalescing-and-512-item-turn-budget.md` — Provider-Level Buffer Contract, PLB-5 | Preserve exactly 512 shared post-acceptance Items with one reserved terminal slot; Item 513 is rejected | AC-9 | Execute that clause's existing mapping in ADR-0005's traceability, including raw fragment/UTF-8/latency boundaries and coalescing-count assertions |
| PLB-6 | `docs/adr/ADR-0005-provider-delta-coalescing-and-512-item-turn-budget.md` — Provider-Level Buffer Contract, PLB-6 | Preserve the independent exact 1,048,576-byte serialized payload cap | AC-9 | Execute that clause's existing mapping in ADR-0005's traceability, including raw fragment/UTF-8/latency boundaries and coalescing-count assertions |
| PLB-7 | `docs/adr/ADR-0005-provider-delta-coalescing-and-512-item-turn-budget.md` — Provider-Level Buffer Contract, PLB-7 | Preserve the declared distinct durable and synchronous diagnostics for resource exhaustion and durability outage without changing SSE terminal shape | AC-9 | Execute that clause's existing mapping in ADR-0005's traceability, including raw fragment/UTF-8/latency boundaries and coalescing-count assertions |
| PLB-8 | `docs/adr/ADR-0005-provider-delta-coalescing-and-512-item-turn-budget.md` — Provider-Level Buffer Contract, PLB-8 | Preserve SSE capacity admitting the legal maximum while all enumerated unrelated bounds stay unchanged | AC-9 | Execute that clause's existing mapping in ADR-0005's traceability, including raw fragment/UTF-8/latency boundaries and coalescing-count assertions |
| PLB-9 | `docs/adr/ADR-0005-provider-delta-coalescing-and-512-item-turn-budget.md` — Provider-Level Buffer Contract, PLB-9 | Preserve provider- and tag-neutral coalescing with exact reasoning-tagged text | AC-9 | Execute that clause's existing mapping in ADR-0005's traceability, including raw fragment/UTF-8/latency boundaries and coalescing-count assertions |
| PC-01 | `koduck-ai/docs/adr/ADR-0006-effective-provider-context-integration.md` — Normative Contract Clauses, PC-01 | Preserve true per-row scope, explicit group source_turn from SQL turn_id, database tuple order, complete nonempty groups, single data query, owned-empty/non-owned outcomes and removal of the flat seam | AC-2, AC-7, AC-9 | Execute that clause's mapped fixture in service ADR-0006 plus new-key requests and retry with inadmissible fresh context |
| PC-02 | `koduck-ai/docs/adr/ADR-0006-effective-provider-context-integration.md` — Normative Contract Clauses, PC-02 | Preserve nonempty groups with pairwise distinct source_turn checked first, empty-collection acceptance with empty-group rejection, CAND-12 scope validation, atomic group preparation before acceptance, and new-Thread empty or continuation snapshot reuse | AC-2, AC-7, AC-9 | Execute that clause's mapped fixture in service ADR-0006 plus new-key requests and retry with inadmissible fresh context |
| PC-03 | `koduck-ai/docs/adr/ADR-0006-effective-provider-context-integration.md` — Normative Contract Clauses, PC-03 | Preserve existing raw count/escaped-byte caps and exact History(ContextLimit) rejection; derived count/bytes never increase, with no second admission | AC-2, AC-7, AC-9 | Execute that clause's mapped fixture in service ADR-0006 plus new-key requests and retry with inadmissible fresh context |
| PC-04 | `koduck-ai/docs/adr/ADR-0006-effective-provider-context-integration.md` — Normative Contract Clauses, PC-04 | Preserve a distinct provider view with one non-correction entry per position, original kind/source identity, exact bytes and no effect authority | AC-2, AC-7, AC-9 | Execute that clause's mapped fixture in service ADR-0006 plus new-key requests and retry with inadmissible fresh context |
| PC-05 | `koduck-ai/docs/adr/ADR-0006-effective-provider-context-integration.md` — Normative Contract Clauses, PC-05 | Preserve existing role and delta flush semantics, post-terminal correction, unchanged no-correction messages and exact current-input placement | AC-2, AC-7, AC-9 | Execute that clause's mapped fixture in service ADR-0006 plus new-key requests and retry with inadmissible fresh context |
| PC-06 | `koduck-ai/docs/adr/ADR-0006-effective-provider-context-integration.md` — Normative Contract Clauses, PC-06 | Preserve current-generation committed Tool rounds, identical history, preserved controls and no new producer/authority | AC-2, AC-7, AC-9 | Execute that clause's mapped fixture in service ADR-0006 plus new-key requests and retry with inadmissible fresh context |
| PC-07 | `koduck-ai/docs/adr/ADR-0006-effective-provider-context-integration.md` — Normative Contract Clauses, PC-07 | Preserve typed causes, exact existing HTTP mapping, zero partial dispatch and payload-free diagnostics | AC-2, AC-7, AC-9 | Execute that clause's mapped fixture in service ADR-0006 plus new-key requests and retry with inadmissible fresh context |
| PC-08 | `koduck-ai/docs/adr/ADR-0006-effective-provider-context-integration.md` — Normative Contract Clauses, PC-08 | Preserve immutable canonical data, adapter-local raw accounting, bounded call-local conversion, borrow release and independent preparations | AC-2, AC-7, AC-9 | Execute that clause's mapped fixture in service ADR-0006 plus new-key requests and retry with inadmissible fresh context |
| CR-01 | `koduck-ai/docs/adr/ADR-0003-correction-item-schema-and-raw-replay.md` — Normative Contract Clauses, CR-01 | Preserve correction's one distinct typed domain and durable representation | AC-4, AC-8, AC-9 | Execute the existing CAND-3 mapped codec/schema/replay checks for that clause and compare canonical rows through the new migration and post-correction retry |
| CR-02 | `koduck-ai/docs/adr/ADR-0003-correction-item-schema-and-raw-replay.md` — Normative Contract Clauses, CR-02 | Preserve the durable target belonging to the same tenant, Thread and Turn and never being self | AC-4, AC-8, AC-9 | Execute the existing CAND-3 mapped codec/schema/replay checks for that clause and compare canonical rows through the new migration and post-correction retry |
| CR-03 | `koduck-ai/docs/adr/ADR-0003-correction-item-schema-and-raw-replay.md` — Normative Contract Clauses, CR-03 | Preserve at most one direct successor per predecessor | AC-4, AC-8, AC-9 | Execute the existing CAND-3 mapped codec/schema/replay checks for that clause and compare canonical rows through the new migration and post-correction retry |
| CR-04 | `koduck-ai/docs/adr/ADR-0003-correction-item-schema-and-raw-replay.md` — Normative Contract Clauses, CR-04 | Preserve raw replay returning every Item once in increasing sequence without mutation or substitution | AC-4, AC-8, AC-9 | Execute the existing CAND-3 mapped codec/schema/replay checks for that clause and compare canonical rows through the new migration and post-correction retry |
| CR-05 | `koduck-ai/docs/adr/ADR-0003-correction-item-schema-and-raw-replay.md` — Normative Contract Clauses, CR-05 | Preserve fail-closed decoding of malformed payloads and structural/self-scope enforcement with no guessed, dropped or rewritten row | AC-4, AC-8, AC-9 | Execute the existing CAND-3 mapped codec/schema/replay checks for that clause and compare canonical rows through the new migration and post-correction retry |
| CR-06 | `koduck-ai/docs/adr/ADR-0003-correction-item-schema-and-raw-replay.md` — Normative Contract Clauses, CR-06 | Preserve additive, idempotent migration that keeps old rows and constraints | AC-4, AC-8, AC-9 | Execute the existing CAND-3 mapped codec/schema/replay checks for that clause and compare canonical rows through the new migration and post-correction retry |
| CA-01 | `koduck-ai/docs/adr/ADR-0004-authenticated-correction-admission.md` — Normative Contract Clauses, CA-01 | Preserve validated owned identity/input, nonblank content, the 65,536-byte limit, no caller sequence and exact bytes | AC-4, AC-8, AC-9 | Execute that clause's mapped check in the existing correction-admission/PostgreSQL targets; submission identity must not alter its records or retry scope |
| CA-02 | `koduck-ai/docs/adr/ADR-0004-authenticated-correction-admission.md` — Normative Contract Clauses, CA-02 | Preserve same-owner terminal-only admission with indistinguishable missing/non-owned target and unchanged lifecycle | AC-4, AC-8, AC-9 | Execute that clause's mapped check in the existing correction-admission/PostgreSQL targets; submission identity must not alter its records or retry scope |
| CA-03 | `koduck-ai/docs/adr/ADR-0004-authenticated-correction-admission.md` — Normative Contract Clauses, CA-03 | Preserve the valid supported earlier current-tip chain and deterministic structural failure | AC-4, AC-8, AC-9 | Execute that clause's mapped check in the existing correction-admission/PostgreSQL targets; submission identity must not alter its records or retry scope |
| CA-04 | `koduck-ai/docs/adr/ADR-0004-authenticated-correction-admission.md` — Normative Contract Clauses, CA-04 | Preserve exact-retry lookup precedence over fresh-write rejection, drift/inconsistent-state rejection and the stored-read cap | AC-4, AC-8, AC-9 | Execute that clause's mapped check in the existing correction-admission/PostgreSQL targets; submission identity must not alter its records or retry scope |
| CA-05 | `koduck-ai/docs/adr/ADR-0004-authenticated-correction-admission.md` — Normative Contract Clauses, CA-05 | Preserve one atomic append and sequence increment with one winner and immutable history | AC-4, AC-8, AC-9 | Execute that clause's mapped check in the existing correction-admission/PostgreSQL targets; submission identity must not alter its records or retry scope |
| CA-06 | `koduck-ai/docs/adr/ADR-0004-authenticated-correction-admission.md` — Normative Contract Clauses, CA-06 | Preserve 4,096 ancestors, the 1-MiB per-read cap, bounded retained state and no full-history read | AC-4, AC-8, AC-9 | Execute that clause's mapped check in the existing correction-admission/PostgreSQL targets; submission identity must not alter its records or retry scope |
| CA-07 | `koduck-ai/docs/adr/ADR-0004-authenticated-correction-admission.md` — Normative Contract Clauses, CA-07 | Preserve at most two 2-second attempts, identity-lock reconciliation and the honest unknown outcome | AC-4, AC-8, AC-9 | Execute that clause's mapped check in the existing correction-admission/PostgreSQL targets; submission identity must not alter its records or retry scope |
| CA-08 | `koduck-ai/docs/adr/ADR-0004-authenticated-correction-admission.md` — Normative Contract Clauses, CA-08 | Preserve rollback before commit, no false cancellation after commit and no detached retries | AC-4, AC-8, AC-9 | Execute that clause's mapped check in the existing correction-admission/PostgreSQL targets; submission identity must not alter its records or retry scope |
| CA-09 | `koduck-ai/docs/adr/ADR-0004-authenticated-correction-admission.md` — Normative Contract Clauses, CA-09 | Preserve other boundaries, terminal append and safe typed diagnostics | AC-4, AC-8, AC-9 | Execute that clause's mapped check in the existing correction-admission/PostgreSQL targets; submission identity must not alter its records or retry scope |
| EP-01 | `koduck-ai/docs/adr/ADR-0005-effective-correction-projection.md` — Normative Contract Clauses, EP-01 | Preserve one expected scope with matching source components, first-mismatching-index scope errors and no inferred or fetched provenance | AC-2, AC-4, AC-7, AC-9 | Execute that clause's existing projection fixture and the PC consumer fixtures; use original raw input for equality while fresh identified requests retain effective context |
| EP-02 | `koduck-ai/docs/adr/ADR-0005-effective-correction-projection.md` — Normative Contract Clauses, EP-02 | Preserve exact raw validation, sequence gaps and inspectable typed causes | AC-2, AC-4, AC-7, AC-9 | Execute that clause's existing projection fixture and the PC consumer fixtures; use original raw input for equality while fresh identified requests retain effective context |
| EP-03 | `koduck-ai/docs/adr/ADR-0005-effective-correction-projection.md` — Normative Contract Clauses, EP-03 | Preserve strictly earlier linear ancestry to a supported root with defined rejection precedence | AC-2, AC-4, AC-7, AC-9 | Execute that clause's existing projection fixture and the PC consumer fixtures; use original raw input for equality while fresh identified requests retain effective context |
| EP-04 | `koduck-ai/docs/adr/ADR-0005-effective-correction-projection.md` — Normative Contract Clauses, EP-04 | Preserve one ordered view per non-correction Item with exact original and selected-source provenance/content and no admission-time content recheck | AC-2, AC-4, AC-7, AC-9 | Execute that clause's existing projection fixture and the PC consumer fixtures; use original raw input for equality while fresh identified requests retain effective context |
| EP-05 | `koduck-ai/docs/adr/ADR-0005-effective-correction-projection.md` — Normative Contract Clauses, EP-05 | Preserve non-text/Tool Items, delta granularity and post-terminal corrections | AC-2, AC-4, AC-7, AC-9 | Execute that clause's existing projection fixture and the PC consumer fixtures; use original raw input for equality while fresh identified requests retain effective context |
| EP-06 | `koduck-ai/docs/adr/ADR-0005-effective-correction-projection.md` — Normative Contract Clauses, EP-06 | Preserve complete result or error, immutable input, deterministic replay and redacted errors | AC-2, AC-4, AC-7, AC-9 | Execute that clause's existing projection fixture and the PC consumer fixtures; use original raw input for equality while fresh identified requests retain effective context |
| EP-07 | `koduck-ai/docs/adr/ADR-0005-effective-correction-projection.md` — Normative Contract Clauses, EP-07 | Preserve call-local O(n) metadata, borrowed content, iterative indexed traversal and no new execution control | AC-2, AC-4, AC-7, AC-9 | Execute that clause's existing projection fixture and the PC consumer fixtures; use original raw input for equality while fresh identified requests retain effective context |
| EP-08 | `koduck-ai/docs/adr/ADR-0005-effective-correction-projection.md` — Normative Contract Clauses, EP-08 | Preserve limiting changes to the declared projection boundary and existing runtime behavior | AC-2, AC-4, AC-7, AC-9 | Execute that clause's existing projection fixture and the PC consumer fixtures; use original raw input for equality while fresh identified requests retain effective context |
| TW-01 | `koduck-ai/docs/adr/ADR-0002-typed-http-wire-serialization.md` — Decision and TW clauses, TW-01 | Preserve the normalized synchronous success body byte-identical to the owned fixture with exact fields and values | AC-1, AC-2, AC-7, AC-9 | Re-execute that clause's mapped golden/parity check; real parsed receipt and unchanged fresh sync/SSE bytes; distinguish the new input's absent/null/value states without altering existing thread_id semantics |
| TW-02 | `koduck-ai/docs/adr/ADR-0002-typed-http-wire-serialization.md` — Decision and TW clauses, TW-02 | Preserve exact SSE framing, order, event names, field order, terminal selection and completed-only usage | AC-1, AC-2, AC-7, AC-9 | Re-execute that clause's mapped golden/parity check; real parsed receipt and unchanged fresh sync/SSE bytes; distinguish the new input's absent/null/value states without altering existing thread_id semantics |
| TW-03 | `koduck-ai/docs/adr/ADR-0002-typed-http-wire-serialization.md` — Decision and TW clauses, TW-03 | Preserve every published item variant's exact fields, nulls, wire names, numbers, escaping and buffered/live byte parity | AC-1, AC-2, AC-7, AC-9 | Re-execute that clause's mapped golden/parity check; real parsed receipt and unchanged fresh sync/SSE bytes; distinguish the new input's absent/null/value states without altering existing thread_id semantics |
| TW-04 | `koduck-ai/docs/adr/ADR-0002-typed-http-wire-serialization.md` — Decision and TW clauses, TW-04 | Preserve exact interrupt and problem JSON shape, status/code/title and UUID correlation identity | AC-1, AC-2, AC-7, AC-9 | Re-execute that clause's mapped golden/parity check; real parsed receipt and unchanged fresh sync/SSE bytes; distinguish the new input's absent/null/value states without altering existing thread_id semantics |
| TW-05 | `koduck-ai/docs/adr/ADR-0002-typed-http-wire-serialization.md` — Decision and TW clauses, TW-05 | Preserve valid outbound JSON with Unicode and the complete control-character range | AC-1, AC-2, AC-7, AC-9 | Re-execute that clause's mapped golden/parity check; real parsed receipt and unchanged fresh sync/SSE bytes; distinguish the new input's absent/null/value states without altering existing thread_id semantics |

CR-07/CR-08 describe the earlier representation slice's excluded consumers and
withdrawal procedure; they do not prohibit later accepted admission/consumer
slices and no withdrawal of CAND-3 occurs here. Those historical scope clauses
are N/A for this slice. TW-06 restricts the earlier serialization refactor to
an internal-only change; it does not prohibit this later explicitly governed
public extension and is N/A as a historical task-scope restriction. The typed
serialization rules and applicable byte/shape/escaping guarantees remain
binding. Each inherited row above retains that clause's existing individual
clause-to-test mapping from its own record; family rows carry every lettered
sub-clause's requirement and per-letter coverage explicitly in their cells.
AC-9 must record those cases' actual
passing results, not substitute a smoke test or uncited suite implication.

## Risk Coverage Matrix [Conditionally Required — source or configuration implementation]

| Risk dimension | Applicability and scenario, or specific N/A reason | Owning boundary | Deterministic verification method | Exact expected result | Acceptance check IDs | Status | Actual evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| concurrency and ordering | P1: overlapping same key, competing content, late binding after early lookup | C-6 transaction and C-2 branch | Four independently connected contenders with barrier and real uniqueness; controlled lookup/preparation ordering | One binding/Turn/input/lease and one creator; matching contenders observe it; drift mutates nothing | AC-3, AC-4, AC-7 | In Progress | 2026-10-09: the four-contender race and drift/scope cases pass on real PostgreSQL; remaining ordering cases pending |
| timeout and deadline | P1: pool reuse/pending COMMIT or preparation exhausts proof time; acceptance deadline expires after Created | C-6 bounded acceptance/SQLx settlement with limited C-2 clock and handoff wiring | Injected manual acceptance clock plus real waits/deferred COMMIT; production-matched pool and writer/proof PIDs for same/other-session paths | Cancellation wins simultaneous shortage; uncancelled write needs at least 4 s; full 2+2 s real settlement survives acceptance-clock advance; declared delayed-COMMIT fixtures prove within 2 s on every realizable session path, unproven failures remain unavailable, and proven Created enters lifecycle | AC-0, AC-5, AC-6 | In Progress | AC-0 Pass on the production-shaped pool with both PID paths; the AC-5 deferred-trigger reconciliation and the AC-6 manual-clock gates pass with real database timers; remaining proof branches pending |
| cancellation and interruption | P1: duplicate disconnect cancels original work; creator disconnect after acceptance or retry after interrupt/fence | C-1 stream negotiation, C-2 branch, C-6 existing recovery | Real Axum body drop and migrated PostgreSQL with blocked provider/control fixtures | Duplicate performs no cancellation/mutation; original interruption/creator cancellation retains one durable terminal and same binding | AC-6, AC-7 | In Progress | Receipt-before-SSE, auth precedence, preparation rejection, and both cancellation branches proven; disconnect and initial-unavailable cases pending |
| resource bounds and backpressure | P2: large input, complete-history receipt reads, retry creates workers or fills output channels | C-6 lookup and C-1/C-2 delivery | Byte-boundary/corrupt input, schema/query review, real pre-stream retry, existing output/buffer regressions | At most one binding/input read; no input duplicate/cache/job or SSE for retry; all existing numeric caps preserved | AC-1, AC-7, AC-8, AC-9 | In Progress | Byte-boundary validation and bounded receipt reads pass; schema/bounds review pending |
| framework or trust-boundary rejection | P1: duplicate JSON identity, missing trust, foreign Thread, hidden duplicate SSE headers | Axum/strict JSON and C-6 ownership predicates | Production router on both routes, strict JSON fixtures, two principals using same UUID | Exact 400/401/404 or independently scoped acceptance; no foreign identity; receipt is 202 JSON before SSE | AC-1, AC-4, AC-7 | In Progress | Exact 400/401 and independently scoped acceptance proven; competing-boundary remainder pending |

## Commit-Fault Fixture And Preflight [Required]

After acceptance and before T-1 production changes, AC-0 uses a test-only
relation in a fresh private schema of the disposable test database, the locked
SQLx 0.9.0 driver, existing Tokio runtime/pool capability, and the same timeout/drop
pattern as production. An `AFTER INSERT FOR EACH ROW` constraint trigger,
declared `DEFERRABLE INITIALLY DEFERRED`, executes `pg_sleep(2.5)` at COMMIT,
while the caller's write attempt times out after two seconds. Reconciliation
acquires through the same pool and takes the same transaction-scoped advisory
key within its full two-second read-only attempt, including pool acquisition,
pending protocol cleanup, transaction setup, lock wait and result read.
Monitor the writer backend to prove it reached COMMIT/trigger execution before
timeout; verify a committed marker after the lock releases and after the
original connection is returned/drained. A `RAISE EXCEPTION` variant must
instead leave no marker.

The primary pool MUST use the inspected `runtime::run` construction,
`PgPoolOptions::new()`, without the existing Harness::connect size override.
For locked SQLx 0.9.0 this means max 10/min 0 connections, acquire timeout 30 s,
idle timeout 600 s, lifetime 1,800 s, fairness and pre-acquire testing enabled,
and no custom acquire/release callbacks. Record/compare effective options with
the source assembly; outer per-attempt 2 s remains authoritative even though
the pool's own acquire timeout is longer. Connect to the test URL, adding only
fixture-local search_path/application identification. No production pool
setting or callback is changed to force a test result. An independent monitor
connection outside this pool must not consume its candidate sessions.

AC-0 records writer and proof backend PIDs for these paths, using
`pg_backend_pid()` in the test-owned transactions and activity/lock observations:

| Pool path | Test-only control with the production-sized pool | Required evidence/result |
| --- | --- | --- |
| Same backend reused | Pre-acquire the other nine sessions and hold them until proof completes; the writer is the sole candidate returned by its dropped transaction | Proof PID equals writer PID; original COMMIT and queued ROLLBACK cleanup precede proof; one durable marker is read within the full 2 s proof budget |
| Different backend acquired | Pre-acquire sessions, release a known other idle session for proof while writer COMMIT is still blocked, and hold remaining candidates | Proof PID differs; proof waits for the writer's key lock to release and reads the same committed marker within its full 2 s budget |

Neither a changed max_connections value nor a requested path label proves
which backend was used. Record actual PIDs, pool occupancy, COMMIT/drop/cleanup
ordering and timings. If the driver retires the writer session or otherwise
makes one path impossible under the unchanged production options, retain an
evidence-backed mechanism/source disposition and exercise the reachable path;
a run that merely happens to use another session cannot establish impossibility.
AC-0 stays non-Pass unless both paths pass or that exclusion is proven. The
deferred-exception variant must also show zero committed marker and no leaked
connection. Record safe server/driver/session conditions; allow the intended
commit delay, and do not copy credentials or user payloads into evidence.

PostgreSQL documents deferred constraint-trigger timing in
[CREATE TRIGGER](https://www.postgresql.org/docs/current/sql-createtrigger.html).
Local SQLx source shows `Transaction::commit` awaits the manager before closing
its transaction state, and Drop calls `PgTransactionManager::start_rollback`,
which queues rollback. That inspection is not proof that a server COMMIT
continues after the future is dropped. AC-0 must demonstrate it on the declared
driver/runtime/database; it is Not Started now. If cancellation aborts the
server commit or the scenario cannot be observed reliably, fail this gate and
revise the verification design through this ADR before T-1, rather than
claiming lost-ACK coverage or silently reverting to lib-only/config fallback.

Once T-1 supplies the migrated relation, AC-5 installs the same deferred
constraint-trigger mechanism on the private schema's `chat_submissions`,
limited to the unique fixture principal/submission. Drive the
normal production acceptance port and SQLx transaction; add no production
fault API or feature. Assert the real binding/Turn/input/lease and creator
outcome after delayed COMMIT, and zero partial acceptance after the exception
variant. Reuse AC-0's pool options and realizable occupancy paths; collect PID
and wait evidence via the test monitor, without adding a production fault hook
or exposing backend identifiers in wire/port outcomes.

Isolation is mandatory, not an alternative to serialization: every AC-0/AC-5
fixture has a generated `cand18_submission_<uuid>` schema and a dedicated pool
whose connection-level `search_path` names only that schema (pg_catalog remains
implicit). Reuse CorruptFixture's schema/options/teardown pattern, but create
the full canonical schema through the real migrations for AC-5, including all
referenced tables; do not fall back to public if a fixture relation is missing.
Qualify trigger/function DDL and assert resolved relation/FK OIDs belong to the
fixture. Never install a trigger, alter a canonical table or change shared
search_path in public. Independent fixtures run under the existing three test
threads without locking each other's chat_submissions relations. On success
and failure, release held sessions, drain/close fixture pools and monitor, then
drop only the generated schema; assert no fixture relations/functions remain.
These triggers are harness artifacts, never startup migrations or deployable
schema. PostgreSQL describes name resolution in
[Schema Search Path](https://www.postgresql.org/docs/current/ddl-schemas.html#DDL-SCHEMAS-PATH)
and backend/session observations in its
[system information functions](https://www.postgresql.org/docs/current/functions-info.html)
and [activity statistics](https://www.postgresql.org/docs/current/monitoring-stats.html#MONITORING-PG-STAT-ACTIVITY-VIEW).
AC-9 still verifies actual integration LCOV and the unchanged gate.

## Acceptance Checks [Required]

T-2 adds submission test modules beneath the existing coverage-selected
`cand_12_projection` (runner/HTTP) and `postgres_cand_11` (real database) test
binaries, using the `cand_18_` test-name prefix. The commands below identify
future tests to be written after acceptance; none is claimed to exist or pass
now. Reuse the migrated database capability; primary commit faults use the
test-database fixture in `Commit-Fault Fixture And Preflight`. A unit double
cannot establish database or Axum guarantees by itself.
These binary names are historical. Place CAND-18 cases in clearly named
submission child modules and use that prefix because the unchanged Sonar
configuration selects these integration entry points; this avoids pretending
that a new target would automatically be measured. This placement does not
make the excluded `--lib` fault tests part of LCOV or weaken the coverage gate.
Each named filter must execute its declared scenario and assertions; an empty
match is not passing evidence. AC-0/AC-5's primary driver/commit-loss evidence
comes from `postgres_cand_11`. Colocated
`cargo test -p koduck-ai --lib cand_18_commit_ack_loss --all-features` tests are
supplementary deterministic edge/fault evidence, not the primary proof of a
server commit surviving dropped acknowledgement and not LCOV coverage.

| Check ID | Subtask | Binary acceptance point | Preconditions or input | Verification method | Exact expected result | Expected evidence | Status | Actual result and evidence |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| AC-0 | T-2 | Deferred COMMIT proves driver timeout/drop and pool reuse behavior before production changes | Accepted ADR; private schema/test-only relation; locked SQLx/runtime; primary pool matches runtime::run defaults; delayed and exception triggers; held-session same/other-backend controls | `cargo test -p koduck-ai --test postgres_cand_11 cand_18_commit_fault_probe --all-features`; normal commit future dropped by 2 s timeout, followed by same-pool read-only proof; capture writer/proof pg_backend_pid plus activity/lock trace | Writer reaches COMMIT/trigger; delayed COMMIT times out locally yet commits one marker; both realizable PID paths read it within a full 2 s proof budget including acquire/protocol cleanup/lock/read; impossible path needs concrete mechanism/source proof, not sampling; exception leaves zero rows and no session leak; production pool options unchanged; schema/trigger/function cleanup succeeds | Actual writer/proof PIDs, matched option snapshot, occupancy and COMMIT/drop/queued-ROLLBACK/proof ordering, timings/rows, any proven exclusion, private schema/OID and cleanup evidence | Pass | 2026-10-09, disposable PostgreSQL 18.6 container: `cand_18_commit_fault_probe_same_backend` proves the writer backend reaches the deferred trigger (`pg_stat_activity` `PgSleep`) before the 2 s write deadline, the timed-out commit survives, and the proof reuses the writer's own PID after COMMIT plus queued-rollback cleanup inside the full 2 s proof budget; `cand_18_commit_fault_probe_other_backend` releases one known non-writer session while the commit is still blocked and proves from a different PID on the same committed marker; `cand_18_commit_fault_probe_aborted_commit` proves a deferred `RAISE EXCEPTION` leaves zero marker rows and every primary session reusable. Primary pool built through the exact `PgPoolOptions::new()` defaults of `runtime::run` plus only fixture `search_path`/`application_name`; fixture OIDs asserted inside the generated schema and cleanup verified per test |
| AC-1 | T-1 | Public and direct input/trust rejection preserves zero mutation | Both routes; absent identity; valid lowercase/uppercase UUID; nil/null/empty/nonstring/nonhyphenated/duplicate/unknown values; malformed JSON; UTF-8 input 0, 65,536, 65,537 bytes; JSON media-type parameters; no/invalid trust; direct invalid command construction | `cargo test -p koduck-ai --test cand_12_projection cand_18_request_validation --all-features`; real Axum router plus typed constructor tests | Only omitted or valid supplied UUID proceeds; invalid bodies return exact 400 problem, identity failures exact 401 plus challenge, no history/provider/Tool work; input cap counts bytes; other existing boundary precedence retained | Parsed documents/headers, canonical zero-write evidence and Red/Green result | Pass | 2026-10-09: `cargo test -p koduck-ai --test cand_12_projection cand_18_request_validation --all-features` passes on the real Axum router — omitted identity legacy success, present null 400, non-hyphenated/braced/simple/nil/empty/nonstring forms 400, duplicate/unknown/malformed 400, 65,536/65,537-byte boundaries, exact `invalid-request` problem; `cand_18_authentication_precedes_lookup_and_effects` proves the exact 401 plus Bearer challenge with zero service invocations; SI-01d's direct-port guard is exercised by the `application::submission` unit tests (`nil_uuid_is_rejected_as_submission_identity`, guard revalidation), enforced before database I/O inside the production adapter, revalidated by the runner before the preliminary lookup (`cand_18_runner_guard_precedes_lookup`, panicking lookup double, typed `InvalidCommand` → 400), and — after review round 2 — enforced at both remaining SI-01d surfaces: the default `TurnService` submission entry fails closed for identified commands (`cand_18_default_service_rejects_identified_commands`, 503 with zero executions) and the direct `submission_observation` port guards before its timed query (`cand_18_observation_entry_guard_precedes_query`, proven instant under an exclusive table lock) |
| AC-2 | T-1 | Fresh identified and legacy acceptance each preserve canonical lifecycle | Migrated isolated PostgreSQL; valid new-Thread and owned-Thread commands with new UUIDs; repeated no-UUID input; deterministic provider and raw replay | `cargo test -p koduck-ai --test postgres_cand_11 cand_18_atomic_acceptance --all-features`; identified real runner cases in `cand_12_projection` | Identified success has exactly one binding, Turn, sequence-1 input and initial lease; existing Thread not replaced; legacy calls have distinct Turn IDs and zero bindings; sync/SSE fresh shapes, prepared effective input, terminal and replay remain exact | Scoped row/constraint assertions, real provider input, HTTP/SSE documents and replay | Pass | 2026-10-09: `cargo test -p koduck-ai --test postgres_cand_11 cand_18_atomic_acceptance --all-features` passes against the migrated disposable database through the production `PostgresTurnHistory` port — first identified acceptance commits exactly one binding/Turn/sequence-1 input/live generation-1 lease (scoped row counts 1/1/1/1), the exact retry returns the identical receipt with row counts unchanged, and legacy unidentified calls still create distinct Turns with zero bindings; existing CAND-1 contract, CAND-12/13 context, and routed suites stay green (491 tests, 0 failures, all targets) |
| AC-3 | T-1 | Same-key contention and lock protocol preserve one durable creator | Four independent connections/runner handles with a barrier; same principal/key/input and None/explicit selector; hash encoding fixtures; held submission and bigint locks; commit not held beyond deadline | `cargo test -p koduck-ai --test postgres_cand_11 cand_18_concurrent_identity --all-features`; inspect live `pg_locks` and hold original provider after acceptance; colocated hash fixtures supplement production connections | Exactly one Created and three identical Existing receipts; one binding/Turn/input/lease and originating execution; complete-scope hash is stable and UUID-case invariant; matching bigint value cannot block the int4-pair submission lock; a submission waiter owns no Item lock; final recheck shares the canonical transaction; simulated shared-key contention changes waiting only; retry finishes while provider blocks and different keys on independent Threads proceed | Typed outcomes, SQL rows/live locks, per-fixture key encoding and provider/actual Tool effect counts | In Progress | 2026-10-09: `cand_18_concurrent_identity` passes — four independently connected contenders with a start barrier resolve to exactly one Created owner and three identical Existing receipts over one binding/Turn/input/lease; SI-03b hash stability and full-scope sensitivity are unit-proven (`submission_lock_keys_are_stable_scoped_and_case_insensitive`). Remaining: live `pg_locks` namespace non-overlap evidence and the provider-held retry case
| AC-4 | T-1 | Exact equality and owner scope are stable across input and lifecycle changes | Same key with changed input bytes, whitespace, Unicode composition or selector; equivalent JSON escapes/order and UUID case; cross-route retry; distinct principals reusing UUID; fresh unknown/foreign Thread; all six lifecycle statuses and later correction | `cargo test -p koduck-ai --test postgres_cand_11 cand_18_equality_and_scope --all-features`; real Axum cases in `cand_12_projection` | Changed semantic input yields exact 409 conflict and zero mutation; representation-only changes yield identical receipt; owners have independent keys; fresh unknown/foreign Thread gives identical 404 shape; original bytes/canonical history and foreign data remain unchanged | Parsed receipts/problems, exact original input/replay and before/after lease/row comparisons | In Progress | 2026-10-09: `cand_18_equality_and_scope` passes — changed input and added explicit selector each return the exact typed 409 with zero mutation, and the same UUID under another subject creates an independent binding; `cand_18_runtime_receipt` proves cross-route 202 identity and the uppercase-UUID acceptance in `cand_18_request_validation`. Remaining: Unicode/whitespace normalization drift, fresh unknown/foreign-Thread 404, six-lifecycle receipts, and post-correction retry cases
| AC-5 | T-1 | Production commit loss, abort, and restart preserve one acceptance and owner | AC-0 Pass; full migrated private schema and per-connection search_path without public; delayed/exception trigger on its chat_submissions; production-matched pool and AC-0's realizable PID paths; creator/input/status/generation/fence/expiry, unavailable/absent/corrupt proof and recreated runner cases | Primary `cargo test -p koduck-ai --test postgres_cand_11 cand_18_commit_reconciliation --all-features` drives normal production port/SQLx/same-pool proof with external backend/lock monitor; `cargo test -p koduck-ai --lib cand_18_commit_ack_loss --all-features` supplements edge cases | Delayed COMMIT after write timeout persists one binding/Turn/input/lease; each realizable same/other-backend path proves this invocation's live Created within full 2 s once; deferred exception leaves zero partial rows; one write/proof in lock order; dead/other creator observes, drift conflicts, unprovable state unavailable/no IDs/effects; restart cannot regain authority; fixture OIDs/FKs isolated, no public trigger/DDL and cleanup succeeds under three test threads | Actual PID/pool/acquire/protocol/COMMIT/proof timing, rows/typed outcome/effects, schema/OID and cleanup trace; lib evidence separate; AC-9 measures production integration LCOV | In Progress | 2026-10-10: the isolated `SubmissionFixture` (private schema, complete canonical migrations, production-shaped pool, tenant-scoped deferred trigger) proves through the production port that the delayed commit whose acknowledgement times out reconciles to the same invocation's live Created owner inside the reserved budgets, and that the deferred `RAISE EXCEPTION` variant leaves zero partial rows and returns unavailable. Remaining: creator-mismatch, fenced/expired/terminal proof branches and the recreated-runner cases |
| AC-6 | T-1 | Injected acceptance clock, cancellation-first gate and known-Created handoff preserve truthful state | Owned manual clock at runner composition; 4 s minus 1 ns/exactly/plus 1 ns; clone/Tool composition/separate starts; invalid earlier-than-start reading; cancellation plus shortage; real pool/locks/staged preparation and unavailable write; test wrapper advances acceptance time past 10 s after real Created; both routes/retry disconnect | `cargo test -p koduck-ai --test postgres_cand_11 cand_18_deadline_and_cancellation --all-features`; exact clock/value and real Axum/runner cases in cand_12_projection; system-clock elapsed/block_on and unchanged real timeout/lease cases, plus AC-5 | Chosen clock retained, budgets per request; pre-write reads clamp to min(2 s, remaining); cancellation wins shortage with 409/zero rows/effects; otherwise less than 4 s or invalid reading yields unavailable/no IDs/zero writes; exactly/above 4 s grants full real 2+2 s without outer cutoff; DB sums 2/6/8/6 s, rejection lookup never also writes; binary settlement assertions prove each full 2 s attempt budget and absence of outer truncation; wall time and scheduler overshoot are recorded evidence, not independent Pass/Fail limits or added retry budget; advancing acceptance clock alters no DB/lease timer; proven Created still executes or actually cancels after 10 s, never deadline-only 503/orphan; unproven commit grants no authority, retry disconnect isolates owner, accepted cancellation retains binding/one terminal | Exact nanosecond budget/state outcomes, assembly/clone/separate-start evidence, system-clock elapsed and real lock/pool/timeout/lease evidence, late-Created handoff and cancellation precedence; per-attempt deadline/cancellation traces prove full write/proof budgets with no outer cutoff; write-plus-proof wall time, scheduler overshoot and test parallelism recorded | In Progress | 2026-10-10: `cand_18_deadline_gate` proves with the staged manual clock and real database/lease timers that less than four remaining seconds starts no write (zero rows, no provider work) and exactly four seconds admits the full real write; `cand_18_invalid_clock_fails_closed` proves the earlier-than-start reading fails closed without resetting the budget; `cand_18_runner_cancellation` proves cancellation-first (SI-07g) and the SI-07h racing durable cancellation with exactly one terminal and no provider execution; policy units pass. Remaining: pre-write read-clamp staged evidence and clone/Tool composition start-separation assertions |
| AC-7 | T-1 | Both production delivery paths return receipt before any stream and never execute retries | Original provider blocked or terminal; real Axum sync/stream retry; poisoned or oversized fresh prior context; initial lookup unavailable with poisoned context; initial lookup absent then concurrent acceptance during rejected preparation; final-lookup unavailability/conflict; controlled stream-body drop | `cargo test -p koduck-ai --test cand_12_projection cand_18_runtime_receipt --all-features`; Axum router backed by production migrated PostgreSQL | Retry is exactly 202 JSON with four fields, same IDs and accepted status for all lifecycle states; no SSE bytes/events, Items/usage, new provider/Tool work or cancellation; initial unavailable returns exact 503 durability-unavailable with no accepted IDs, prior-history preparation, acceptance write or effects; late exact binding returns receipt, late drift 409, absent binding original rejection, final unavailable 503; fresh creator follows old stream decision | Real response status/headers/complete body, preparation/provider/effect counts, zero acceptance attempts on initial unavailable, canonical before/after state | In Progress | 2026-10-09/10: `cand_18_runtime_receipt` proves the exact 202 JSON receipt (four members, canonical UUID text, `accepted`) on both routes before any SSE header, and the drifted 409 problem exposing no identities; `cand_18_preparation_rejection` proves the rejected-preparation final unlocked lookup preserves the original rejection with zero writes and provider work. Remaining: initial-unavailable 503 and the controlled stream-body drop |
| AC-8 | T-1 | Migration, lookup integrity, and binding lifetime preserve canonical evidence | Populated migrated database; apply migration 0010 twice through startup sequence; corrupt/mismatched owner/Turn/input/type/oversized input fixture; terminal, correction, restart and lease expiry; canonical deletion in a rolled-back integrity test | `cargo test -p koduck-ai --test postgres_cand_11 cand_18_migration_and_integrity --all-features`; schema/query inspection at recorded source revision | New scoped primary/foreign/unique constraints exist; no backfill or changed old rows; corrupt structure yields unavailable with no authority; bounded read loads one input only; no duplicate input/output column, TTL/cache/job; binding survives all retained-canonical transitions and cannot become an orphan | Semantic schema/query inspection, row/payload snapshots, constraint violations and lifetime retry outcomes | In Progress | 2026-10-10: `cand_18_migration_and_integrity` applies the exact production startup sequence twice (and a third time) on the isolated populated fixture — legacy rows unchanged with no backfill, one binding survives, and the identified acceptance works only after 0010. 2026-10-10 review round 1: `cand_18_oversized_stored_input_fails_unavailable` proves a stored sequence-1 payload above the 65,536-byte bound classifies as unavailable — never a semantic conflict — through both the lookup and the transaction recheck. Remaining: individual constraint-rejection cases and lifetime-retry statuses |
| AC-9 | T-2 | Focused checks, preservation, structure, and latest-revision delivery gates all pass | Accepted ADR; AC-0 Pass before T-1; completed T-1; disposable migrated PostgreSQL via `KODUCK_AI_TEST_DATABASE_URL`; exact implementation revision and one PR to dev | Focused `cargo test -p koduck-ai --test cand_12_projection cand_18_ --all-features` and `cargo test -p koduck-ai --test postgres_cand_11 cand_18_ --all-features`; all routed commands and structural/contract review below | AC-0 through AC-8 and applicable risk rows Pass; existing preservation checks pass; production delayed-commit/proof branches actually appear in integration LCOV with required 80% changed-line coverage and matching Sonar admission; engineering limits/required CI/exact-revision review pass, no unresolved blocking thread | Preflight and Red/Green reports, invariant mapping, production source/LCOV measurements, command/CI/analysis/review results and exact reviewed SHA/round | Not Started | AC-0 through AC-8 partial evidence recorded; changed-line LCOV measured 92.3% locally across the three selected integration targets before the latest test additions; the gate, CI correspondence, and revision-bound review await the completed T-2 cases |

Implementation verification from repository root:

```sh
npm test --prefix tools/governance-validator
npm run validate --prefix tools/governance-validator
cargo fmt --all --check
cargo clippy -p koduck-ai --all-targets --all-features -- -D warnings
cargo test -p koduck-ai --all-targets --all-features
python3 tools/sonarqube/gate.py check --revision HEAD
```

Use a disposable migrated PostgreSQL database; tests MUST fail rather than
skip when its declared fixture is unavailable. The Sonar command may instead
use matching successful pre-push evidence under the canonical hook workflow.
Retain safe command/test evidence and remove disposable compiler/database
output. Source/configuration PR readiness requires the Scope Routing commands'
required CI correspondence; local Sonar is the recorded exception to CI.
Record the latest reviewed SHA/result and round, including any explicit bounded
owner extension. Fixes/pushes, approval and resuming the same work do not reset
that budget. The round-7 drafting extension recorded in Supporting Notes has
been consumed; any further agent-driven review requires a new bounded owner
extension recorded there. Unresolved
findings or absent exact-revision coverage prevent readiness.

For this documentation-only drafting task, run only the two governance
commands, a whitespace check, and structured semantic/acceptance review.
Rust/PostgreSQL/Sonar/CI/review coverage of the future implementation remains
Not Started; a passing document validator is not acceptance evidence for SI
behavior or formal approval.

Allowed final check statuses are `Pass`, `Fail`, or `N/A — <specific reason>`.
`Fail` blocks completion. None of AC-0 through AC-9 is optional for this slice.

## Completion Checklist [Required]

| ID | Item | Completion Criterion | Expected Evidence | Status | Actual Evidence |
| --- | --- | --- | --- | --- | --- |
| A-1 | ADR approved | Eligible non-author approval, time and exact Approve recorded | Active metadata | Pass | `Approve` recorded from Decision Owner @linhai at 2026-10-09T14:26:36Z in the context identifying this record, after the Q-2/Q-3/Q-4 determinations below; author/drafting agent did not approve |
| A-2 | Complete task delivered | T-1/T-2 Complete; AC-0 through AC-9 Pass with source and outcome evidence | Implementation/acceptance rows | Not Started | Pending — implementation not started |
| A-3 | Reciprocal ADD link synchronized | CAND-18 links this exact ADR and this ADR names that ADD/candidate; candidate Complete only when ADR Complete/Verified | ADD row, ADR metadata and immutable evidence | In Progress | Selection and reciprocal paths drafted together; final lifecycle not reached |
| A-4 | Requirement levels satisfied | Required sections complete for the current lifecycle stage, conditional triggers assessed, optional content accurate | Structured review | Pass | Q-2/Q-3/Q-4 owner determinations recorded 2026-10-09T14:26:36Z; bounded rounds 1–7 complete with round 7 on blob `911501ab73a7768e803412bc9cae2b467f888a87`; approval revision validated by the governance commands recorded in Current Verification |
| A-5 | Acceptance checks decidable | Each row identifies subtask, inputs, method, exact result and evidence, including production boundaries | Structured acceptance review | Pass | AC-0 through AC-9 reviewed through bounded rounds 1–7; owner determinations recorded 2026-10-09T14:26:36Z without changing any check; checks remain executable before implementation begins |
| A-6 | Engineering exceptions governed | No unsupported exception; final file/unit/nesting review satisfies Constraints | Engineering Exceptions and AC-9 evidence | Not Started | Pending — no exception proposed; final source review after implementation |
| A-7 | Contracts and baseline risks covered | Every SI/inherited applicable clause mapped; all five applicable risk rows Pass before readiness/completion | Traceability, risk rows and tests | Not Started | Pending — traceability designed, execution not started |
| A-8 | Governance validation passed | No required-section, lifecycle, index, link, template or Mermaid diagnostic | Governance command results | Pass | `npm run validate --prefix tools/governance-validator` passed; drafting verification below records the supporting checks |

## Supporting Notes [Optional]

### Current Review Disposition

| Review context | Current record |
| --- | --- |
| Conversation and supplied report | Codex conversation `01a11f6f-f094-74d3-9695-a843a7586dae`; latest four suggestions concern ADR input blob `d62eb2c8c20cd00ec5533b01b1625ad08e8b7e48` |
| Historical passes | Local rounds 1–6, inputs/results and earlier findings remain in Change Log; their assessments do not cover this corrected revision |
| Owner extension | Consumed — direct reply `@linhai Extend review by one round`, recorded at 2026-10-09T10:16:03Z, authorized the four round-7 dispositions and one corrected-draft pass, completed at 2026-10-09T10:19:48Z; remaining agent review rounds: zero |
| Authorization boundary | These suggestions only; no count reset, eighth round, formal ADR approval, implementation or Q-2/Q-3/Q-4 determination; earlier clarifications retained in Change Log |
| Round 7 result | Completed by @codex at 2026-10-09T10:19:48Z on corrected ADR input blob `911501ab73a7768e803412bc9cae2b467f888a87`; checked the four dispositions against SI-07c/d/i, traceability, AC-6 and approval history. No additional technical drafting finding in this scope; actual approval/audit remains a future owner gate |
| Approval/lifecycle gates | Closed — Q-2/Q-3/Q-4 determinations recorded from @linhai at 2026-10-09T14:26:36Z, followed by the owner's explicit `Approve` in the same context; the brief Context approval summary is the paragraph beneath the Open Questions table. Decision Status is `Accepted`; AC-0/implementation remain Not Started with AC-0 first; reciprocal Selected link retained |
| Future formal approval audit | Satisfied — the distinct approval Change Log entry below cites the owner's 2026-10-09T14:26:36Z response and back-references the owner clarification at 2026-10-09T08:22:13Z. The historical round-3 reply remains non-approval history; active metadata follows AGENTS.md's canonical Approval and Status contract |

| Latest finding | Disposition | Evidence or remaining gate |
| --- | --- | --- |
| AC-6 hard wall-time ceiling may fail under scheduler load | Addressed in draft | Wall time/overshoot/parallelism are recorded evidence; binary checks retain complete real 2+2 s budgets without an outer cutoff, and SI-07c/SI-07d's budgets and gate are unchanged |
| Elapsed 6 s may imply a scheduled start | Addressed in draft | SI-07d's path table explicitly says a permitted write MUST start no later than elapsed 6 s with at least 4 s remaining |
| Historical Approve may be mistaken for formal acceptance | Closed by actual approval; audit entry recorded | The distinct approval Change Log entry cites the owner's 2026-10-09T14:26:36Z response with the back-reference to the owner clarification at 2026-10-09T08:22:13Z; the historical log is preserved and active approval metadata carries the new evidence |
| Clock contract repeated in design section | Addressed in draft | SI-07i owns reading/retention/timer-isolation semantics; the design section cites that authority and retains composition/fixture mechanics only |
| Acceptance-section review budget wording stale after round 7 | Addressed from user-provided finding | Explicitly states the round-7 extension is consumed and further agent review requires a new recorded bounded owner extension; no additional agent review pass |

### Observed Pre-Existing Size Debt

The round-3 line-span audit of every touched file found two executable units
above the non-waivable 80-line limit that predate this slice and whose lines
this PR does not modify: `SqlxPostgresExecutor::append_async`
(`koduck-ai/src/adapters/history/postgres/sqlx_executor.rs`, 87 physical
lines) and `SqlxPostgresExecutor::reconcile_expired_async` (same file, 98
physical lines), both from earlier accepted slices. Per the
software-engineering standard's incremental-adoption rule they are recorded
here as observed debt and left for a scoped remediation task; every unit
this PR adds or modifies stays within the limit.

### Current Verification

| Drafting check | Result | Current evidence |
| --- | --- | --- |
| `npm test --prefix tools/governance-validator` | Pass | Acceptance-recording and implementation-recording revisions on 2026-10-09: exit 0; 208 passed, 0 failed, 0 skipped each run |
| `npm run validate --prefix tools/governance-validator` | Pass | Exit 0; Governance validation passed for the acceptance-recording revision and again for the implementation-recording revision (In Progress statuses, AC evidence columns, and the index row) |
| `cargo fmt --all --check` | Pass | Implementation revision on 2026-10-09: exit 0 |
| `cargo clippy -p koduck-ai --all-targets --all-features -- -D warnings` | Pass | Zero findings on the implementation revision |
| `cargo test -p koduck-ai --all-targets --all-features` | Pass | 2026-10-10: 517 passed, 0 failed (four duplicate in-module `#[test]` registrations superseded by the binary's wrapper tests during the round-6 module split; every scenario still executes exactly once) across every target with `KODUCK_AI_TEST_DATABASE_URL` pointing at the disposable migrated PostgreSQL 18 container, including the full `cand_18_` suite: probes, real-Axum validation/receipt, atomic/retry/drift/scope, runner flows/cancellation/tool/stream, deadline gate and invalid clock, preparation rejection, default-port fail-closed, commit reconciliation, migration idempotence, and the real-run assembly |
| `cargo llvm-cov` (three selected integration targets) | Pass | Changed-line coverage measured 92.3% (674/730) against the dev merge base before the final test additions; the delivery-gate measurement reruns at AC-9 |
| `git diff --check`; new-file `git diff --no-index --check -- /dev/null docs/adr/ADR-0018-chat-submission-identity-and-atomic-acceptance.md` | Pass | No whitespace diagnostics on 2026-10-09; tracked check exit 0, no-index exit 1 is the expected new-file difference |
| Structured semantic/acceptance review | Pass | Owner-authorized bounded round 7 completed at 2026-10-09T10:19:48Z; the four dispositions are consistent. Q-2/Q-3/Q-4 determinations were recorded at 2026-10-09T14:26:36Z and the approval revision below re-ran the governance commands |

A-1/A-4/A-5 are Pass; A-2 remains Not Started pending implementation. Validator
success and the bounded technical assessment recorded the owner determinations
and approval without implementing anything. Rust/PostgreSQL acceptance,
SQLx commit-drop preflight, Sonar, implementation CI and revision-bound review
have not run. AC-0 through AC-9 remain
Not Started. The ADD stays Current and CAND-18 stays Selected; this correction
does not create an observation candidate or revise the approved ADD design.

## Archival [Conditionally Required — Decision Status is `Rejected`, or Decision Status is `Deprecated` or `Superseded` and Implementation Status is final]

This record is live (`Accepted`, Implementation Status `Not Started`) and is
not archival-eligible now. On a future legal
retirement, archive it in `docs/adr/archive/` with the same filename, update
all governed-file markers, reciprocal ADD paths and the index in one change,
and preserve reciprocal supersession paths when applicable. These inactive
future-lifecycle instructions do not establish present approval or completion.

## Change Log [Required]

| Date | Change | Author |
| --- | --- | --- |
| 2026-10-09 | Drafted one project Full ADR from Current ADD-0001 CAND-18 on `codex/cand-18-submission-identity-adr`, based on local dev `a0f81b54ca13eb8133c364caf1edb3dfa1c6c96d`. Added proposed SI-01 through SI-10, two coupled subtasks, contract traceability, five baseline risks, invariant matrix, binary acceptance checks and migration/rollback boundaries. Created reciprocal candidate/index links together. Remains Proposed/Not Started; no source/configuration implementation or formal approval occurs in this change. | @codex |
| 2026-10-09 | Local structured review round 1 examined ADR draft blob `63f1d2c02a1dfb08b03b2c5d29ff1c163f7a13ad`, ADD blob `e6f6ebb4e09cb136b15f5f9195a7fb132fdf2165`, and index blob `2793a88c09e5a76037a519f00a3935b60ad9240b`. Corrected direct-port validation and typed successful receipt propagation, distinguished broken joins from absent bindings, specified selector/non-nil schema integrity, corrected inherited contract anchors and completed preservation mappings (including PLB-8/PLB-9), and made the production commit-loss test command and nonempty named-filter evidence explicit. Distinguished cancellation observed before the acceptance write from cancellation racing a commit or occurring after acceptance, with exact wire/state checks in AC-6. These pre-commit document identities are local review context, not pushed-revision review coverage or approval. | @codex |
| 2026-10-09 | Local structured review round 2 examined ADR blob `6c0f8d2492f6d2e1a62f00400862a646c5e46664`, ADD blob `e6f6ebb4e09cb136b15f5f9195a7fb132fdf2165`, and index blob `2793a88c09e5a76037a519f00a3935b60ad9240b`: checked lifecycle-stage requirements, scope/links, contract traceability, coupled subtasks, baseline risks, acceptance methods, source touchpoints, creator authority, commit settlement, cancellation and migration/rollback. Result at that time: no unresolved local drafting finding; 208 governance tests, repository validation and whitespace checks passed. Later user reports supersede that assessment for later drafts. These local blobs identify historical review inputs, not approval or pushed-revision coverage; approval/implementation gates stayed pending. Historical Supporting Notes evidence was consolidated here. | @codex |
| 2026-10-09 | Revised the draft in response to the user-provided six approval findings and five supporting suggestions on blob `c15fd8c9e6c17c3f47a4efa413104fdc2d14fede`. Corrected build_router; explicitly assessed excluded lib coverage and retained the unchanged admission gate; moved Q-2/Q-3/Q-4 to pending @linhai determinations; disclosed lost-output recovery limits and a proposed separate observation candidate; split clause IDs/traceability; specified whole-path timing, unlocked observations, distinct advisory namespace/full-scope hashing/lock order; added inherited wire/projection records, selector/null guidance, test-placement rationale and a sequence diagram. Historical round-2 review does not cover this revision; no new agent review round, approval, implementation, ADD design revision, coverage-config edit, commit or push occurs. | @codex |
| 2026-10-09 | Recorded deterministic verification of the review corrections: all 208 governance tests pass, repository governance validation passes, and tracked/new-file whitespace checks report no diagnostics. A-4/A-5 remain In Progress because the revised draft needs current review and the material owner determinations remain pending; successful validation does not settle them. | @codex |
| 2026-10-09 | Responded to user-provided follow-up findings on blob `07f4f93c87a1785449a429554c19b812e29393b7`: made 10 s an admission clock with a remaining-4-s write gate and full 2+2 s settlement, preserved known Created handoff, synchronized diagram/matrix/AC-6, split SI-02a..f and SI-08a..d, moved primary commit-loss/rollback evidence into selected postgres_cand_11 via disposable deferred constraint triggers, and added mandatory AC-0 SQLx/drop preflight before T-1. Updated Input Audit/TN-4 and consolidated Supporting Notes to one current verification table. No owner identity/extra-round authorization, Q-2/Q-3/Q-4 determination, runtime proof, new review pass, approval, source/config change or ADD design revision is inferred. | @codex |
| 2026-10-09 | Recorded repository-owner authorization received as `@linhai Approve` in response to the specifically bounded extra-round question in Codex conversation `01a11f6f-f094-74d3-9695-a843a7586dae`, at 2026-10-09T07:58:30Z. The owner self-declared @linhai and authorized exactly one additional local structured review of this corrected draft, round 3. This is review-budget evidence, not ADR approval, Q-2/Q-3/Q-4 determination, source authorization, or a reset of the review count. | @codex |
| 2026-10-09 | Completed authorized local structured review round 3 at 2026-10-09T08:03:04Z: ADR input blob `fa33272bcb384a6653a69b679c8d3b85e4ea30b5`, ADD blob `e6f6ebb4e09cb136b15f5f9195a7fb132fdf2165`, index blob `2793a88c09e5a76037a519f00a3935b60ad9240b`. Reviewed current-stage requirements, single boundary/reciprocal selection, individual clause/check inputs, full settlement reservation and proven-Created handoff, deferred-trigger integration evidence and AC-0 stop gate, trust/lifetime/preservation/rollback. Clarified the trigger's explicit DEFERRABLE INITIALLY DEFERRED declaration and test-database fixture ownership. No further technical drafting defect identified; material Q-2/Q-3/Q-4 still pending, A-4/A-5 In Progress and AC-0 runtime proof Not Started. These are local draft identities, not pushed-SHA coverage; no fourth round, acceptance, source/config change or delivery is authorized. | @codex |
| 2026-10-09 | Owner @linhai replied `@linhai Extend review by one round` to the explicitly bounded clarification question in the same conversation, recorded at 2026-10-09T08:22:13Z. This clarifies the earlier round-3 extension and authorizes the latest supplied report, in-scope remediation and one corrected-draft pass as round 4. Earlier `@linhai Approve` interpretations above are retained as history, not used as active approval evidence or a normative exception; this explicit owner clarification now governs review scope. No fifth round, formal ADR approval, Q-2/Q-3/Q-4 determination or source implementation is authorized. | @codex |
| 2026-10-09 | Corrected latest supplied findings on ADR blob `ebef8a7e49422c4360c103f528ee7dac18ff47c1`: added consumer-owned monotonic clock/budget injection and system/manual implementations to SI-07i, plan/touchpoints/AC-6; distinguished policy composition from database-fault APIs; specified cancellation before time rejection; matched AC-0/AC-5 to runtime pool defaults and required actual same/other-backend PID evidence or proven exclusion; fixed private-schema/search_path isolation and cleanup under concurrent targets. Compressed current review context into tables and retained historical process here. Owner questions remain unresolved and all runtime acceptance checks unexecuted. | @codex |
| 2026-10-09 | Completed owner-authorized bounded round 4 at 2026-10-09T08:32:48Z: corrected ADR input blob `ffe5c6354e0cd6e73f7017f80151771260d23659`, ADD blob `e6f6ebb4e09cb136b15f5f9195a7fb132fdf2165`, index blob `2793a88c09e5a76037a519f00a3935b60ad9240b`. Reviewed the supplied clock/injection, pool/PID, isolation, cancellation-priority, review-authorization and readability findings with their individual plan/trace/check mappings. Clarified owned duration-from-origin readings and limited guaranteed within-budget proof to declared commit fixtures, preserving truthful unavailable for other unproven failures. No further technical drafting finding within this pass's scope; Q-2/Q-3/Q-4 unresolved, AC-0/AC-5/AC-6 runtime checks Not Started, A-4/A-5 In Progress. Local identities are not pushed-SHA review coverage or formal approval; review count is four and no fifth round is authorized. | @codex |
| 2026-10-09 | Owner @linhai replied `@linhai Extend review by one round`, recorded at 2026-10-09T09:34:49Z in the same conversation. Authorizes round 5 only for this report's acceptance naming/C-6 boundary correction and one follow-up pass; no sixth round, formal ADR approval, implementation or Q-2/Q-3/Q-4 determination. Renamed active AdmissionClock/AdmissionInstant/AdmissionBudget, builder and proposed file path to Acceptance equivalents; synchronized SI-07c/d/i, diagram, plan, matrices and AC-6, assigned the deadline to C-6 bounded atomic acceptance/true SI-06 settlement with limited C-2 wiring, and explicitly left CAND-17 Thread admission unaffected. Historical names/evidence preserved; no behavior or ADD design change. Context approval summary deferred until owner determinations as requested. | @codex |
| 2026-10-09 | Completed authorized bounded round 5 at 2026-10-09T09:36:22Z on ADR input blob `d5cb53b370864d24e4ffb92385be717a367567ad`, unchanged ADD blob `e6f6ebb4e09cb136b15f5f9195a7fb132fdf2165` and index blob `2793a88c09e5a76037a519f00a3935b60ad9240b`. Checked acceptance type/builder/path names, SI-07c/d/i, sequence/matrix/AC-6 mappings, C-6 deadline ownership with limited C-2 support and unchanged CAND-17 exclusion. No additional technical drafting finding within this scope; 10 s write-start deadline, full 2+2 s proof, cancellation priority and known-Created handoff unchanged. Q-2/Q-3/Q-4 unresolved, A-4/A-5 In Progress; no formal approval or sixth-round authorization. | @codex |
| 2026-10-09 | Owner @linhai replied `@linhai Extend review by one round` to the explicitly bounded question in this conversation, recorded at 2026-10-09T09:53:09Z. Authorizes round 6 only for the four drafting findings on ADR blob `3045cf46a37daf0fe9ea3264a5268803a16e8e3d` and one corrected-draft pass; no count reset, seventh round, formal approval, implementation or Q-2/Q-3/Q-4 determination. Clarified SI-07a initial-unavailable 503/no preparation or write with invariant/trace/AC-7 coverage; disambiguated SI-03b complete-encoding SHA-256; aligned AC-6's 5 s wall-time ceiling and 1 s maximum measurement tolerance; moved the duplicated CAND-17 clarification into Constraints. Existing design, budgets, ADD and status unchanged. | @codex |
| 2026-10-09 | Completed owner-authorized bounded round 6 at 2026-10-09T09:55:20Z on corrected ADR input blob `fca550aa16615fc273738c6889ae3a4ae69349e6`, unchanged ADD blob `e6f6ebb4e09cb136b15f5f9195a7fb132fdf2165` and index blob `2793a88c09e5a76037a519f00a3935b60ad9240b`. Checked SI-07a's initial-unavailable branch against the diagram, invariant, traceability and explicit AC-7 input/result/evidence; SI-03b's full ordered digest against the existing scope/lock checks; AC-6's aligned wall-time ceiling without new retry budget; and the single CAND-17 exclusion with Constraints clarification. No additional technical drafting finding within this scope. Q-2/Q-3/Q-4 unresolved, A-4/A-5 In Progress, runtime checks Not Started; these local identities are review inputs, not pushed-SHA coverage or formal approval. Count remains six; no seventh round authorized. | @codex |
| 2026-10-09 | Owner @linhai replied `@linhai Extend review by one round` to the explicitly bounded question in this conversation, recorded at 2026-10-09T10:16:03Z. Authorizes round 7 only for these four suggestions on ADR input blob `d62eb2c8c20cd00ec5533b01b1625ad08e8b7e48` and one corrected-draft pass; no count reset, eighth round, formal approval, implementation or Q-2/Q-3/Q-4 determination. AC-6 wall time becomes recorded evidence, with full 2+2 s budget/no-outer-truncation binary checks; clarified the elapsed-6-s latest permitted write start; made SI-07i the clock authority and limited its design section to composition/fixtures; tracked an explicit reference to the 2026-10-09T08:22:13Z clarification for the future actual approval entry. Runtime policy budgets, scope, ADD and lifecycle statuses unchanged. | @codex |
| 2026-10-09 | Completed owner-authorized bounded round 7 at 2026-10-09T10:19:48Z on corrected ADR input blob `911501ab73a7768e803412bc9cae2b467f888a87`, unchanged ADD blob `e6f6ebb4e09cb136b15f5f9195a7fb132fdf2165` and index blob `2793a88c09e5a76037a519f00a3935b60ad9240b`. Checked AC-6's recorded timing versus full-budget/no-outer-truncation assertions and unchanged SI-07c/d gate/cancellation/Created semantics; latest permitted write-start wording; SI-07i authority with composition-only design and retained clock checks; and future approval back-reference to the explicit owner clarification. No additional technical drafting finding within this scope. Governance tests: 208 passed, 0 failed, 0 skipped; repository validation and tracked/new-file whitespace checks passed. Q-2/Q-3/Q-4 remain unresolved, A-4/A-5 In Progress and runtime checks Not Started. Count is seven; no eighth round or formal approval authorized. | @codex |
| 2026-10-09 | Corrected the user-provided stale review-budget wording finding on ADR input blob `12085d3adcc58f9e6189bd6ef847d308e3d7a8cf`: the acceptance section and current owner-extension record now explicitly say round 7 is consumed, with zero remaining agent review rounds and a new recorded bounded owner extension required for further agent review. This is a targeted correction based on the supplied finding and existing round-7 evidence, not an additional agent review pass; count remains seven. Governance tests: 208 passed, 0 failed, 0 skipped; repository validation and tracked/new-file whitespace checks passed. No approval, owner-question determination, source or ADD design change. | @codex |
| 2026-10-10 | Automatic-review round 7 (Codex, reviewed revision `01627cf6b2550b6884d8b3b67abd257cfdd52575`) reported two P2 findings, both confirmed genuine. The repository owner @linhai authorized one further remediation cycle plus round-8 automatic review coverage in the governing conversation at 2026-10-10T11:03:16Z. Both were fixed test-first: (1) the `TurnHistory::submission_observation` default now runs `validate_identified_command` and fails closed for invalid identified commands before reporting absence (SI-01d), proven by `cand_18_default_observation_entry_guards_commands`; (2) `require_owned_explicit_thread` now selects the owned row `FOR SHARE`, holding it until the acceptance transaction commits so the Thread cannot be deleted or re-owned between the check and final acceptance (SI-03a), with `cand_18_explicit_thread_row_locks_through_acceptance` proving a concurrent row writer blocks the acceptance for its full write budget and the released row then accepts. The delete-race window itself cannot be deterministically seeded without the production fault hook this record prohibits, so the guarantee is structural through the held share lock and that limitation is disclosed here. Each original thread received its own reply citing the fixing revision before resolution. Verification: fmt clean, clippy `-D warnings` zero findings, 517 tests across all targets with 0 failures, governance validator 208/0 and validation passed. Round 8 is the owner-authorized final automatic-review round; further iteration needs a new bounded owner extension. | @codex |
| 2026-10-10 | Automatic-review round 6 (Codex, reviewed revision `966e98a072f9b0ad4e40a773fa21d297ac4e6a9d`) reported one P2: `tests/postgres_cand_11/submission.rs` reached 1,875 physical lines, past the 1,800-line test-file hard limit this record refuses to except. The repository owner @linhai authorized one further remediation cycle plus round-7 automatic review coverage in the governing conversation at 2026-10-10T06:00:05Z. The module was decomposed into the focused child modules the Implementation Plan's affected paths anticipated: `submission/acceptance.rs` (296 lines: atomic acceptance, equality and scope, contention, explicit-selector existence), `submission/runner.rs` (507: provider doubles, identified flows, cancellation branches, tool/stream preservation, entry guards), `submission/deadline.rs` (342: manual clock, write gate, invalid reading, rejected preparation, port defaults), and `submission/integrity.rs` (654: migration idempotence, deferred-trigger reconciliation, payload bounding, bounded reads, runtime assembly), with the shared helpers staying in a 131-line root — every file below the 1,000-line decomposition-review threshold and far below the hard limit. The split also removed four duplicate `#[test]` registrations whose functions were already executed through the binary's wrapper tests, so every scenario now runs exactly once (515 passed, 0 failed). Verification: fmt clean, clippy `-D warnings` zero findings, governance validator 208/0 and validation passed. Round 7 is the owner-authorized final automatic-review round; further iteration needs a new bounded owner extension. | @codex |
| 2026-10-10 | Automatic-review round 5 (Codex, reviewed revision `0db9f04285dfb9c7281929e7e02291296515fcdd`) reported two P2 findings, both confirmed genuine. The repository owner @linhai authorized one further remediation cycle plus round-6 automatic review coverage in the governing conversation at 2026-10-10T05:26:16Z. Both were fixed test-first: (1) the `TurnHistory::prior_thread_turns_bounded` default now fails closed with `Unavailable` instead of delegating to an unbounded read that could exceed the supplied deadline and the ten-second acceptance budget (SI-07c), proven by `cand_18_bounded_read_default_fails_closed` with an unbounded read that panics if reached; (2) `payload_codec.rs` exceeded the 800-line production-file hard limit (810) after the slice's decoder addition, so the submission-specific `decode_original_user_input` moved to its only consumer `submission_child.rs` (786 lines after the move; no engineering exception invoked or needed). Each original thread received its own reply citing the fixing revision before resolution. Verification: fmt clean, clippy `-D warnings` zero findings, 519 tests across all targets with 0 failures, governance validator 208/0 and validation passed. Round 6 is the owner-authorized final automatic-review round; further iteration needs a new bounded owner extension. | @codex |
| 2026-10-10 | Automatic-review round 4 (Codex, reviewed revision `a8783c0ef037090ac95e43b63c60db199467f100`) reported two P2 findings, both confirmed genuine. The repository owner @linhai authorized one further remediation cycle plus round-5 automatic review coverage in the governing conversation at 2026-10-10T05:01:29Z. Both were fixed test-first: (1) the legacy runner entries (`execute`, `execute_with_observer`, `execute_with_observer_and_cancellation` — one shared base) now reject an identified command with the new typed `TurnCommandError::SubmissionOnLegacyEntry` surfaced as `TurnRunError::InvalidCommand` (the existing 400 invalid-request mapping) instead of silently executing it through the unidentified path, proven by `cand_18_legacy_entries_reject_identified_commands` with zero bindings written; (2) every binding read deserializes its stored payload through the existing ADR-0001 strict-JSON duplicate-member rejection before parsing, so `{"content":"other","content":"expected"}` fails unavailable even when the surviving member matches, proven by `cand_18_duplicate_member_payload_fails_unavailable`. Each original thread received its own reply citing the fixing revision before resolution. Verification: fmt clean, clippy `-D warnings` zero findings, 518 tests across all targets with 0 failures, governance validator 208/0 and validation passed. Round 5 is the owner-authorized final automatic-review round; further iteration needs a new bounded owner extension. | @codex |
| 2026-10-10 | Automatic-review round 3 (Codex, reviewed revision `a7a5c6dcec5584a82287fff636d3320a7d26e2bd`) reported three P2 findings, all confirmed genuine. The repository owner @linhai authorized one further remediation cycle plus round-4 automatic review coverage in the governing conversation at 2026-10-10T04:22:01Z. All three were fixed test-first, together with a self-audit that brought every other unit this PR introduces below the non-waivable 80-line limit: (1) fresh prior-history reads now flow through the new deadline-aware `prior_thread_turns_bounded` port with the runner clamping to its remaining acceptance time (SI-07c), proven by `cand_18_bounded_history_read_honors_deadline` under an exclusive lock; (2) every binding read bounds the stored payload in SQL through a 400,000-byte transport envelope (server-side `CASE` returning `NULL`, never transferred) and rejects noncanonical single-member structure before comparison (SI-09), proven by `cand_18_noncanonical_and_oversized_payloads_fail_unavailable`; (3) `execute_identified_submission` was decomposed into `accept_identified_fresh`, `identified_prepared_history`, and `finish_created_identified`, with the adapter's `require_owned_explicit_thread`/`insert_binding`/`read_proof_row`/`started_with_live_initial_lease` helpers and the over-limit tests split (the 305-line validation case table became clause-scoped tests with a shared per-case helper). Two pre-existing over-limit units untouched by this PR (`append_async`, `reconcile_expired_async`) are recorded as observed debt in Supporting Notes. Verification: fmt clean, clippy `-D warnings` zero findings, 516 tests across all targets with 0 failures, governance validator 208/0 and validation passed. Round 4 is the owner-authorized final automatic-review round; further iteration needs a new bounded owner extension. | @codex |
| 2026-10-10 | Automatic-review round 2 (Codex, reviewed revision `f3d90da8083b58c2b7e2b493057f1369784f9ae4`) reported two P2 findings, both confirmed as genuine new SI-01d violations on distinct entry points. After the two-round agent budget was exhausted, the repository owner @linhai authorized one further remediation cycle plus round-3 automatic review coverage in the governing conversation at 2026-10-10T03:45:18Z. Both findings were fixed test-first: (1) the default `TurnService::execute_submission_controlled` now fails closed with `durability-unavailable` for any identified command instead of silently executing it as a fresh unidentified submission (proven by `cand_18_default_service_rejects_identified_commands` with zero service executions); (2) the direct `SqlxPostgresExecutor::submission_observation` entry validates the identified command before its timed query (proven by `cand_18_observation_entry_guard_precedes_query`, which holds an exclusive `chat_submissions` lock and observes instant typed rejection while an unguarded query would consume its full deadline). Each original thread received its own reply citing the fixing revision before resolution. Verification: fmt clean, clippy `-D warnings` zero findings, 512 tests across all targets with 0 failures, governance validator 208/0 and validation passed. Round 3 is the owner-authorized final automatic-review round; any further iteration requires a new bounded owner extension. | @codex |
| 2026-10-10 | Automatic-review round 1 of the implementation (Codex, reviewed revision `927c1d13cfc6080f651760da9eb2d607d08c2c70`) reported three P2 findings; all three were confirmed actionable against the accepted contract and fixed test-first with new regression cases: (1) the identified runner ran the preliminary lookup before the SI-01d direct-command guard — the guard now runs first in `execute_identified_submission`, returning the new typed `TurnRunError::InvalidCommand` that the presentation boundary maps to the existing 400 invalid-request problem, proven by `cand_18_runner_guard_precedes_lookup` against a panicking lookup double; (2) the identified acceptance auto-created a caller-selected explicit unknown Thread — the transaction now requires an existing owned row for every explicit selector and returns the indistinguishable typed `NotFound` without claiming the key, proven by `cand_18_explicit_thread_must_exist`; (3) `decode_original_user_input` accepted stored content above the 65,536-byte input bound and misclassified it as semantic drift — the decoder now fails unavailable before any comparison, proven by `cand_18_oversized_stored_input_fails_unavailable` through both lookup and recheck. Each finding received its own original-thread reply citing the fixing revision before resolution. Verification: fmt clean, clippy `-D warnings` zero findings, 510 tests across all targets with 0 failures, governance validator 208/0 and validation passed. This is implementation review round 1; the follow-up automatic review of the fixing revision is round 2, the last without a new bounded owner extension. | @codex |
| 2026-10-10 | Completed the T-2 verification wave: restored serialization for the real-time database tests (the AC-0 probe proof budgets are host-load-sensitive; a self-deadlock from double-locking wrappers was fixed by locking only inside the submission/probe functions), split the runner and deadline tests into focused units under the 100-line limit, and added the remaining production-boundary cases — the AC-5 isolated deferred-trigger reconciliation fixture (delayed commit proven Created on the same invocation; abort leaves zero rows), the AC-6 manual-clock write gate at exactly four seconds and the SI-07i fail-closed earlier reading, SI-07g/SI-07h cancellation branches through the real runner, SI-07b rejected preparation, the SI-01d default-port fail-closed behavior, AC-8 startup-sequence idempotence on an isolated populated fixture, the tool-denial and stream-ended preservation cases, and the real-`run` assembly through `apply_startup_migrations` (made public for the acceptance entry) and `compose_production_runner` (extracted, documented, and covered). `apply_startup_migrations` is now `pub`; no behavior changed. Full verification: fmt clean, clippy `-D warnings` zero findings, 507 tests across all targets with 0 failures, governance validator 208/0 and validation passed; changed-line LCOV 92.3% across the three selected targets before the final additions. AC-5/AC-6/AC-7/AC-8 moved to In Progress with recorded evidence; AC-3 pg_locks, AC-4 remainder, AC-5 remaining proof branches, AC-7 initial-unavailable, AC-8 constraint attempts, and AC-9 delivery gates remain open. | @codex |
| 2026-10-09 | Began T-1/T-2 implementation after acceptance, test-first per the plan. AC-0 preflight passed first: three `cand_18_commit_fault_probe` cases on a disposable PostgreSQL 18 container prove the delayed deferred-trigger commit survives the dropped two-second acknowledgement on both realizable pool-PID paths within full proof budgets and that the abort variant leaves zero rows and no leaked session, with the production `PgPoolOptions::new()` defaults unchanged. T-1 delivered the core contract: `SubmissionId`, the acceptance clock/budget values, `TurnCommand.submission_id` with a three-state wire visitor distinguishing absent/null/value, the SI-03b scoped hash lock keys with fail-closed direct-port guards, `TurnHistory` identified-observation/acceptance ports with fail-closed defaults, migration 0010 (`chat_submissions`), the SQLx identified lookup/atomic acceptance/reconciliation child module reusing the canonical write logic and bounded settlement, the runner identified flow (unlocked lookup, preparation, cancellation-first remaining-4-second gate, proven-Created handoff, SI-07h durable cancellation), HTTP 202 receipt/conflict mapping before SSE, runtime registration and explicit system clock, the `cand-18-chat-submission-v1.md` contract copy, and governed-file markers. `ports.rs` exceeded the 800-line limit during the extension and was decomposed by moving its cohesive model Tool-call cluster into `ports/tool_calls.rs` (845 → 686 lines; no engineering exception invoked). Executed verification: `cargo fmt --all --check` clean; `cargo clippy -p koduck-ai --all-targets --all-features -- -D warnings` zero findings; 491 tests across all targets with the migrated disposable database, 0 failures — including the new `cand_18_request_validation`, `cand_18_authentication_precedes_lookup_and_effects`, `cand_18_runtime_receipt` (real Axum), and `cand_18_atomic_acceptance`, `cand_18_concurrent_identity`, `cand_18_equality_and_scope` (real migrated PostgreSQL) cases. Subtask statuses and AC rows updated truthfully; AC-3/AC-4/AC-7 remain In Progress and AC-5/AC-6/AC-8/AC-9 Not Started. Implementation Status became `In Progress`; index row synchronized. | @codex |
| 2026-10-09 | Recorded the repository owner's three determinations and formal acceptance. In the governing conversation that unambiguously identified this record, @linhai responded at 2026-10-09T14:26:36Z accepting the proposed resolutions of Q-2 (optional opt-in under SI-01a preserving unidentified chat), Q-3 (SI-05's constant 202 receipt for active and terminal Turns without output replay or stream resubscription; owned observation/replay deferred to the proposed new ADD candidate), and Q-4 (SI-08 retention for the canonical Turn's lifetime with no TTL, cleanup, or new deletion capability). In the same context the owner then responded with the exact case-sensitive word `Approve`, which is recorded as Approver @linhai, Approval Time 2026-10-09T14:26:36Z, and Approval Evidence `Approve`; this entry is the distinct approval audit record back-referencing the owner clarification at 2026-10-09T08:22:13Z, and the historical round-3 reply remains non-approval history. Decision Status changed from `Proposed` to `Accepted`; the Submission Contract is now authoritative and T-2's AC-0 preflight is the next authorized implementation step. No source, configuration, ADD design, or runtime change occurs in this recording change; the Open Questions table, Submission Contract preamble, Supporting Notes, Completion Checklist, and Archival trigger wording were synchronized. | @codex |
