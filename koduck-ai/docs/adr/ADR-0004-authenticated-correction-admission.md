# ADR-0004: Authenticated Correction Admission

## Metadata [Required]

- **Decision Status**: Accepted
- **Implementation Status**: Verified
- **Date**: 2026-09-04
- **Author**: @codex
- **Decision Owner**: @linhai
- **Required Approver**: @linhai
- **Record Scope**: Service internal — koduck-ai
- **Approver [Conditionally Required — Decision Status is or has been `Accepted`]**: @linhai
- **Approval Time [Conditionally Required — Decision Status is or has been `Accepted`]**: 2026-09-05T07:51:32+08:00
- **Approval Evidence [Conditionally Required — Decision Status is or has been `Accepted`]**: Approve
- **Rejector [Conditionally Required — Decision Status is `Rejected`]**: N/A — Proposed
- **Rejection Time [Conditionally Required — Decision Status is `Rejected`]**: N/A — Proposed
- **Rejection Evidence [Conditionally Required — Decision Status is `Rejected`]**: N/A — Proposed
- **Retired By [Conditionally Required — Decision Status is `Deprecated` or `Superseded`]**: N/A — Proposed
- **Retirement Time [Conditionally Required — Decision Status is `Deprecated` or `Superseded`]**: N/A — Proposed
- **Retirement Evidence [Conditionally Required — Decision Status is `Deprecated` or `Superseded`]**: N/A — Proposed
- **Retirement Reason [Conditionally Required — Decision Status is `Deprecated` or `Superseded`]**: N/A — Proposed
- **Blocked From [Conditionally Required — Implementation Status is `Blocked`]**: N/A — Not Started
- **Blocker And Evidence [Conditionally Required — Implementation Status is `Blocked`]**: N/A — Not Started
- **Blocker Owner [Conditionally Required — Implementation Status is `Blocked`]**: N/A — Not Started
- **Blocker Exit Or Recheck Criterion [Conditionally Required — Implementation Status is `Blocked`]**: N/A — Not Started
- **Related [Optional]**: [Trello requirement](https://trello.com/c/4WI4sszw); `koduck-ai/docs/adr/ADR-0003-correction-item-schema-and-raw-replay.md`; `docs/adr/ADR-0001-provider-neutral-turn-kernel.md`; `docs/adr/ADR-0005-provider-delta-coalescing-and-512-item-turn-budget.md`
- **Architecture Source [Conditionally Required — product demand]**: `docs/architecture/ADD-0001-ai-service-codex-alignment.md` — CAND-11
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

CAND-3 already supplies `ItemPayload::Correction`, strict durable decoding,
ordered raw replay, and the same-scope foreign key and single-successor unique
index. It deliberately supplies no correction admission operation. Reusing
ordinary foreground append would fail on a terminal Turn and would omit caller
ownership, predecessor currency, and caller-stable retry identity.

This ADR selects CAND-11 from the Current ADD and defines one internal C-6
transaction exposed through an owned application port. It permits a later
correction without reactivating its original Turn. CAND-12 will interpret
correction chains; CAND-13 will supply the effective history to providers.
This record does not implement either consumer or expose a client endpoint.

## Scope [Required]

In scope:

- An owned correction command, result/error contract, and persistence port.
- PostgreSQL ownership, terminal-state, predecessor-chain, stable-identity,
  sequence, single-winner, deadline, and commit-reconciliation behavior.
- Focused semantic tests and real-PostgreSQL concurrency/failure checks.
- Minimal module declarations, re-exports, and governance evidence needed for
  that one persistence boundary and one implementation pull request to `dev`.

Out of scope:

- Routes, REST/SSE schemas or events, UI, authentication middleware, provider
  input construction, effective projection, forks, checkpoints, and compaction.
- Memory, Multitask, Tool authority, lease lifecycle, deployment, new dependencies,
  configuration, migration changes, or rewriting existing Items.
- A generic post-terminal append API, caller-selected sequence, reusable
  approval grants, or background correction retry workers.

## Tensions, Constraints, And Open Questions [Required]

### Identified Tensions [Conditionally Required — competing goals or trade-offs exist]

| ID | Tension | Impact | Decision |
| --- | --- | --- | --- |
| TN-1 | A terminal Turn is immutable as execution state but its content may be corrected later. | Ordinary append must retain terminal rejection. | Add a separate correction transaction; preserve the terminal, lease, and original Items. |
| TN-2 | A client may retry after a commit acknowledgement disappears. | A new identity or premature absence result can duplicate or misreport the write. | Bind retries to the correction Item ID and exact request; serialize reconciliation behind that identity. |
| TN-3 | A long correction chain must be validated without unbounded work. | Complete integrity validation competes with latency and memory limits. | Use scoped bounded ancestor reads and an explicit admission limit; never load complete Thread history. |

### Constraints [Required]

- CAND-3 CR-01 through CR-08 remain authoritative within their declared scope:
  CR-01 through CR-06 govern representation, schema, codec, and raw replay;
  CR-07 excludes admission and consumer integration from the CAND-3 slice,
  without prohibiting this separate admission decision; CR-08 preserves
  correction rows and a verified reader/schema pair during recovery.
- CAND-1 owns authenticated `TrustContext`, canonical sequence allocation,
  terminal and lease state, and the two bounded database attempts used for
  write/acknowledgement reconciliation. CAND-11 adds no foreground authority.
- Project ADR-0005's 512-Item and 1-MiB execution-output limits remain unchanged.
  This post-terminal operation has its own explicit admission bounds below;
  it does not restart or consume a live provider-output budget.
- `0001_cand_1_history.sql` through `0009_cand_3_correction_items.sql` remain
  immutable. Existing schema objects suffice; no migration is proposed.
- The common engineering and Rust standards apply. Tests assert behavior,
  structured outcomes, and database state rather than source/document wording.

### Open Questions [Conditionally Required — material questions exist or were resolved during drafting]

None — the selected ADD fixes the task boundary. The limits, error categories,
and transaction design below are explicit proposed decisions for @linhai to
approve, not assumed existing product contracts.

## Decision Drivers [Required]

1. **Ownership**: untrusted target identifiers cannot grant cross-subject access.
2. **Immutable evidence**: correction appends one Item and preserves raw history.
3. **Deterministic retries**: a durable exact retry returns its original result.
4. **Atomicity**: admission and sequence allocation have one database owner.
5. **Bounded failure**: every database wait and chain traversal has a limit.

## Options Considered [Required]

### Option: Extend ordinary foreground append

Pros:

- Reuses its entry point and lease checks.

Cons:

- Needs a terminal-state bypass and lacks correction ownership/retry semantics.
- Couples post-terminal edits to expired foreground leases.

### Option: Validate outside a transaction, then insert

Pros:

- Short insert code and apparently less lock contention.

Cons:

- Predecessor currency, status, and sequence can change before insertion.
- A unique-index error alone cannot establish exact retry semantics.

### Option: A dedicated owned correction port and one guarded transaction

Pros:

- Isolates C-6 invariants and reuses durable representation and locking.
- Can be tested through the production database boundary.

Cons:

- Serializes competing corrections within one Turn.
- Introduces a small owned command/error surface.

## Decision [Required]

**Selected option**: A dedicated owned correction port and one guarded transaction.

**Rationale**: Ownership, current-tip admission, sequence allocation, and retry
identity must be decided together. The existing schema already provides the
needed structural constraints; a separate port preserves ordinary append's
terminal/lease contract without introducing another data owner.

### Normative Contract Clauses [Required]

- **CA-01 — Owned command**: `CorrectionCommand` carries an authenticated
  `TrustContext`, `ThreadId`, `TurnId`, caller-stable correction `ItemId`,
  predecessor `ItemId`, and replacement content. The caller allocates the
  correction identity once and retains it across retries. No sequence or
  payload discriminator is caller-controlled. Content must contain non-whitespace
  text and be at most 65,536 UTF-8 bytes, measured before serialization, with
  accepted bytes preserved exactly. Invalid content returns `InvalidContent`
  before database access; an identity equal to its predecessor returns
  `InvalidPredecessor` before database access.
- **CA-02 — Ownership and terminal admission**: Every write, retry lookup, and
  reconciliation verifies tenant, subject, Thread, and Turn together against
  `threads` and `turns`. Missing and non-owned targets both return `NotFound`
  without exposing the other owner's state. Only `completed`, `failed`,
  `interrupted`, and `cancelled` admit a new correction. `started` and
  `recovery-pending` return `TurnNotTerminal`. A successful correction leaves
  status, terminal Item, interrupt flags, and all lease fields unchanged.
- **CA-03 — Current valid chain**: A new correction targets an existing Item in
  the same tenant/Thread/Turn with sequence below the Turn's current
  `next_sequence` and no direct successor. A root must be `UserMessage` or
  `AgentMessageDelta`;
  a `Correction` predecessor is permitted only if every ancestor is in scope,
  strictly earlier, and terminates at such a root. Each chain node has at most
  one direct successor. Missing/cross-scope/self/unsupported predecessors return
  `InvalidPredecessor`; an otherwise valid predecessor with an existing direct
  successor returns `PredecessorConflict`. Malformed durable payload, broken
  ancestor links, cycles, nondecreasing ancestor order, branches, or invalid
  sequence state return `CorruptHistory`. Existing raw-replay semantics do not
  change. Replacement targets one Item; grouping agent deltas into a provider
  message remains CAND-12/CAND-13 work.
- **CA-04 — Exact identity retry**: After ownership verification and before
  new-write `TurnNotTerminal` or current-tip rejection, look up the
  caller-stable correction identity in its tenant. An existing correction with the exact Thread, Turn, predecessor,
  and replacement bytes returns its original durable Item, including sequence,
  with no write, even if a later correction now succeeds it. Any existing
  tenant-scoped identity with different scope, kind, predecessor, or content
  returns `IdentityConflict` without returning the conflicting row. Identities
  in different tenants are independent. Malformed matching stored data returns
  `CorruptHistory`, never guessed success. An exact stored match on a
  nonterminal Turn also returns `CorruptHistory`: lawful admission creates
  corrections only after termination, and a terminal Turn never reactivates.
  Thus this combination is inconsistent durable state, not a new-write
  `TurnNotTerminal` rejection. Identity mismatch still takes `IdentityConflict`
  precedence; only an absent identity proceeds to new-write state checks.
  All stored retry reads remain subject to the CA-06 stored-payload read cap.
- **CA-05 — Atomic append**: One transaction serializes the operation identity,
  locks the subject-owned Turn, validates admission, allocates its current
  `next_sequence`, inserts exactly one Correction Item with
  `is_terminal = false` using the CAND-3 codec, and advances `next_sequence`
  by exactly one. The sequence must be positive, greater than all existing
  Turn sequences, and incrementable within
  PostgreSQL BIGINT; otherwise return `CorruptHistory` without mutation.
  Existing Items, including the terminal, are never updated or deleted.
  Competing fresh identities for one tip yield exactly one success and the
  rest `PredecessorConflict` when the database remains available within the
  deadline; under the same deadline condition, identical concurrent requests
  return the same one durable Item. Outside that condition, CA-07 governs
  timeout and reconciliation outcomes; single-successor uniqueness still holds.
  Distinct chains in the same Turn receive distinct increasing sequences.
- **CA-06 — Bounded admission**: Validate at most 4,096 existing ancestor nodes,
  including the predecessor and root. A chain of exactly 4,096 valid nodes is
  permitted; observing a 4,097th node returns `ResourceLimit`. Reads use bounded
  scoped queries; they must not accumulate full Thread history or unbounded
  payloads. Inspect payload byte lengths before loading content; any ancestor
  or retry payload above 1,048,576 stored bytes returns `ResourceLimit` before
  fetching that payload. The 1-MiB read cap guards pre-existing or corrupt
  rows; new replacement content is bounded more tightly by CA-01. Even
  six-byte JSON escaping of every allowed content byte plus the fixed
  correction object envelope remains below the read cap. The limits measure
  different representations and are not interchangeable.
  Limit retained ancestor data to one decoded payload plus bounded
  identity/sequence metadata. These admission limits do not
  truncate or change raw replay and do not alter provider history limits.
- **CA-07 — Deadlines and ambiguous acknowledgement**: Pool acquisition,
  locks, queries, and commit belong to one 2-second write-attempt budget. On
  unavailable transport or timeout, cancel/drop the write future and perform
  at most one separate 2-second read-only reconciliation attempt. It acquires
  the same operation-identity transaction lock before observing the durable
  result, then repeats ownership and exact-identity checks. A matching durable
  Item returns success; proven absence after the writer has settled returns
  `NotApplied`; conflicting identity returns `IdentityConflict`. If writer
  settlement, ownership lookup, or durable state cannot be established within
  the budget, return `Unavailable` with commit outcome unknown. Never describe
  `Unavailable` as zero mutation, automatically reissue the write, allocate a
  new identity, or schedule an unbounded worker.
- **CA-08 — Cancellation and truthful failure**: A database attempt cancelled
  before COMMIT rolls back, including sequence allocation. Cancellation or
  connection loss racing COMMIT follows CA-07; an already committed correction
  is not undone or reported as a proven rejection. Dropping the caller need
  not deliver a response, but must not leave an application-owned retry loop
  or detached write future. This synchronous internal port introduces no new
  client interrupt API and never changes the original Turn's terminal state.
  `InvalidContent`, `InvalidPredecessor`, `NotFound`, `TurnNotTerminal`,
  `PredecessorConflict`, `IdentityConflict`, `CorruptHistory`, `ResourceLimit`,
  and `NotApplied` make no new durable change for that invocation; an exact
  retry may observe a previous invocation's existing Item.
- **CA-09 — Boundary preservation and diagnostics**: Ordinary foreground
  append still rejects terminal Turns, including after a correction. Correction
  writes do not publish SSE, issue provider requests, substitute raw history,
  or mutate D-6/D-7, lease, Memory, or Multitask state. Errors are owned typed
  categories; diagnostics may record the category and safe correlation IDs,
  but never replacement/original content, credentials, raw SQL parameter
  values, or raw database error text containing them.

### Transaction And Reuse Design [Required]

Expose `CorrectionStore::correct` with owned `CorrectionCommand`, `Item`, and
`CorrectionError` types from `application/correction_store.rs`, following the
existing `approval_store.rs` and `attempt_store.rs` port naming. Implement it on
`SqlxPostgresExecutor` in a correction-specific child module. The application
port owns the I/O contract; PostgreSQL owns transactional admission. This
requires no generic extension to every `TurnHistory` implementation and no
production route or runtime configuration change.

Validate command-local fields first. Acquire `commit_reconciliation::lock_operation`
for the correction identity before locking any Turn, matching the existing
write/reconciliation order. The advisory lock key is derived by that existing
helper from the caller-stable correction `ItemId` UUID; there is no separate
operation-ID type or newly allocated lock identity. Lock only the authenticated
Turn row using its joined Thread ownership predicate. All correction paths use the same order;
reconciliation locks the identity and performs read-only ownership/result
checks without waiting for a second write attempt.

After command validation, check ownership before any row-disclosing result.
Look up the caller identity before the new-write terminal-state check. For an
existing identity, return CA-04 identity conflict first, or validate an exact
match (including its terminal-state invariant) before returning success or
corruption. Perform this before testing predecessor currency or new-write
traversal limits (the CA-06 stored retry-payload cap still applies). For a fresh
identity, check terminal state, then sequence validity, then predecessor/ancestor validity
and limits, and finally whether the valid tip already has a successor. This
order defines error precedence when more than one rejection condition exists.
Walk the ancestor chain under the Turn lock with parameterized scoped queries.
Query metadata and byte lengths before payload contents. Use `DurableItemCodec`,
existing payload decoding, `insert_item`, the existing unique indexes, and
foreign keys; do not copy the codec or weaken constraints. Verify every conditional write's affected-row
count. Failed validation drops/rolls back the transaction. Unexpected SQL errors
remain typed unavailability unless a named invariant is positively established;
an arbitrary unique-violation message must not be guessed as a tip conflict.

Mirror the existing `settle_commit_attempt` two-budget policy in a focused
correction settlement function. The current helper is a private implementation
detail of the `postgres` module, not a general exported settlement API, and its
result and reconciliation futures are specifically typed to `HistoryError`.
Rust permits descendant modules to access that private ancestor item, so its
visibility does not itself prevent a call from `sqlx_executor/correction.rs`.
The decisive reason for a separate helper is preserving the richer correction
error contract without generalizing the existing helper, widening its
visibility, or changing unrelated callers solely to share control-flow lines.
Reconciliation checks the full request binding, not merely presence of the Item ID. It cannot return proven absence
while the original writer still holds its operation lock.

### Consequences [Required]

Positive: authenticated corrections become durably usable as a foundation for
later projection without losing original evidence or weakening live append.

Negative: competing corrections serialize per Turn; extremely long or oversized
legacy chains are rejected by admission; transport loss may still leave an
explicitly unknown outcome after the bounded reconciliation window.

Mitigations: bounded indexed lookups, exact retry identity, explicit typed
outcomes, real database race tests, and preservation of raw replay for diagnosis.

## Implementation Plan [Required]

**Complete task outcome**: One authenticated internal correction operation
appends or retrieves exactly one correctly scoped successor, or returns its
typed rejection/unknown outcome, with all CA-01 through CA-09 checks passing.

**Primary implementation boundary**: C-6 persistence and data behavior in the
`koduck-ai` PostgreSQL correction transaction.

Allowed subtask statuses: `Not Started`, `In Progress`, `Blocked`, `Complete`,
or `N/A — <specific reason>`.

| ID | Objective or deliverable | Included scope | Status | Actual implementation evidence |
| --- | --- | --- | --- | --- |
| T-1 | Define the owned correction command, port, and errors. | Private-field command validation, documented application exports, exact byte preservation, focused semantic tests. | Complete | Implemented in commit `849b0c2ffa722c70484e1516ab1e122ac3f8ca9c`: `koduck-ai/src/application/correction_store.rs` exposes `CorrectionCommand`, `CorrectionStore`, `CorrectionError`, and `MAX_CORRECTION_CONTENT_BYTES`; AC-1 passes. |
| T-2 | Implement and verify atomic correction admission. | Dedicated production PostgreSQL operation, scoped ancestor validation, exact retries, bounded settlement, real database acceptance and risk checks. | Complete | Implemented in commit `849b0c2ffa722c70484e1516ab1e122ac3f8ca9c`: `koduck-ai/src/adapters/history/postgres/sqlx_executor/correction.rs` implements `CorrectionStore` for `SqlxPostgresExecutor`; AC-2 through AC-5 pass against the isolated test database, and AC-6 passes. |

**Affected paths**: `koduck-ai/src/application/correction_store.rs` (new),
`koduck-ai/src/application/mod.rs` (declaration/re-exports),
`koduck-ai/src/adapters/history/postgres/sqlx_executor/correction.rs` (new),
`koduck-ai/src/adapters/history/postgres/sqlx_executor.rs` (module declaration),
focused child test modules where private fault seams are needed,
`koduck-ai/tests/cand_11_correction_admission.rs` (new),
`koduck-ai/tests/postgres_cand_11.rs` (new), and the existing shared PostgreSQL
fixture module only if minimal fixture reuse requires it. Governance evidence
is limited to this ADR, its index, and the selected ADD candidate.

### Stable Implementation Touchpoints [Conditionally Required — source or configuration implementation]

The represented existing source revision for every existing symbol below is
`8e432bd60b56f754f188eda57a60866374b8e8e2`. New symbols are planned and do not yet
exist at that revision. Stable symbols are evidence anchors, not layout tests.

| Path | Stable symbol or contract anchor | Key code excerpt, when needed | Purpose | Source revision |
| --- | --- | --- | --- | --- |
| `koduck-ai/src/application/correction_store.rs` | Planned `koduck_ai::application::CorrectionCommand`, `koduck_ai::application::CorrectionStore::correct`, `koduck_ai::application::CorrectionError` | N/A — owned symbols express the boundary | Validate inputs and expose the narrow C-6 port. | Planned against `8e432bd60b56f754f188eda57a60866374b8e8e2` |
| `koduck-ai/src/adapters/history/postgres/sqlx_executor/correction.rs` | Planned `koduck_ai::application::CorrectionStore::correct` implementation for `koduck_ai::adapters::history::postgres::SqlxPostgresExecutor` | N/A — trait implementation is the anchor | Own admission, transaction, and bounded settlement. | Planned against `8e432bd60b56f754f188eda57a60866374b8e8e2` |
| `koduck-ai/src/adapters/history/postgres/sqlx_executor.rs` | `koduck_ai::adapters::history::postgres::SqlxPostgresExecutor`; `koduck_ai::adapters::history::postgres::sqlx_executor::{insert_item,is_terminal_status}` | N/A — stable symbols suffice | Reuse pool/runtime and canonical insert; add only a child-module declaration. `insert_item` and `is_terminal_status` are `pub(super)`, accessible within the `postgres` subtree including the planned child; do not widen visibility or re-export them. | `8e432bd60b56f754f188eda57a60866374b8e8e2` |
| `koduck-ai/src/adapters/history/postgres/commit_reconciliation.rs` | `koduck_ai::adapters::history::postgres::commit_reconciliation::lock_operation` | N/A — stable symbol suffices | Reuse writer/reconciler identity serialization without changing existing behavior. | `8e432bd60b56f754f188eda57a60866374b8e8e2` |
| `koduck-ai/src/adapters/history/postgres.rs` | `koduck_ai::adapters::history::postgres::{settle_commit_attempt,DurableItemCodec}` | N/A — stable symbols suffice | Private descendant-accessible `settle_commit_attempt` is a policy reference, not an exported generic API; `DurableItemCodec` remains reusable. No parent behavior or visibility change. | `8e432bd60b56f754f188eda57a60866374b8e8e2` |
| `koduck-ai/src/domain/item_correction.rs` | `koduck_ai::domain::item_correction::{ItemCorrection::new,validate_raw_replay}` | N/A — stable symbols suffice | Reuse existing representation and verify unchanged raw replay. | `8e432bd60b56f754f188eda57a60866374b8e8e2` |
| `koduck-ai/migrations/0009_cand_3_correction_items.sql` | `turn_items_one_direct_correction`, `turn_items_correction_scope`, `turn_items_correction_shape` | N/A — schema object names suffice | Existing single-successor and same-scope constraints; read-only dependency. | `8e432bd60b56f754f188eda57a60866374b8e8e2` |

**Migration and rollback strategy [Conditionally Required — this replaces or
changes existing behavior]**: No schema migration is required. Before promotion,
withdraw the new correction port implementation if verification fails. Preserve
all committed Items and the CAND-3 schema/codec; ordinary append remains
unmodified. Any later deployed rollback uses a separately accepted OCR and a
verified artifact that can replay correction rows; never remove constraints or
rows to restore availability. This ADR authorizes no deployment operation.

### Engineering Exceptions [Conditionally Required — an engineering rule is exceeded or waived]

N/A — no engineering rule is waived. Place new logic in the correction boundary;
keep existing large modules to declarations/re-exports. At implementation,
measure each affected executable unit against the 80-line hard limit and each
file against the applicable review/exception thresholds before requesting
review. No suppression marker may stand in for that assessment. An unexpected
exception requires an ADR revision and reapproval before retaining it.

## Contract-To-Check Traceability [Conditionally Required — source or configuration implementation]

Each CA clause is authoritative in this ADR's Normative Contract Clauses section.

| Clause ID | Authoritative contract path and heading | Exact normative requirement | Acceptance check or deterministic test IDs | Explicit coverage method |
| --- | --- | --- | --- | --- |
| CA-01 | This ADR — Normative Contract Clauses | Validated owned identity/input, nonblank content, 65,536-byte limit, no caller sequence, exact bytes. | AC-1 | Boundary and multibyte fixtures, private-field API/compiler check, zero database calls on local rejection. |
| CA-02 | This ADR — Normative Contract Clauses | Same-owner terminal-only admission, indistinguishable missing/non-owned target, unchanged lifecycle. | AC-2, AC-4 | Ownership/status matrix, cross-owner retry/reconciliation, before/after lifecycle rows. |
| CA-03 | This ADR — Normative Contract Clauses | Valid supported earlier current-tip chain and deterministic structural failure. | AC-2 | Every kind, missing/foreign target, repeated chains, corrupt ancestor fixtures, exact error assertions. |
| CA-04 | This ADR — Normative Contract Clauses | Exact retry lookup precedes fresh-write state/tip rejection; inconsistent exact-match state and identity drift are rejected; stored-read cap applies. | AC-2, AC-3, AC-4, AC-5 | Exact/mismatched existing identities on nonterminal fixtures, retry after another successor, field drift, cross-tenant identity, acknowledgement faults, and retry-payload read limits; AC-2 explicitly decodes malformed stored retry records and requires CorruptHistory. |
| CA-05 | This ADR — Normative Contract Clauses | One atomic append and sequence increment, one winner, immutable history. | AC-2, AC-3, AC-5 | Replay/row equality, concurrency barriers, independent chains, sequence overflow/corruption, transaction rollback. |
| CA-06 | This ADR — Normative Contract Clauses | 4,096 ancestors and 1-MiB per-read cap; bounded retained state and no full history. | AC-5, AC-6 | Exact-limit/one-over fixtures, instrumented query byte/row counts, source inspection of allocation ownership. |
| CA-07 | This ADR — Normative Contract Clauses | At most two 2-second attempts, identity-lock reconciliation, honest unknown outcome. | AC-4 | Fake-time settlement plus real PostgreSQL lock/commit faults and proven-absence checks. |
| CA-08 | This ADR — Normative Contract Clauses | Rollback before commit; no false cancellation after commit, no detached retries. | AC-4, AC-5 | Cancel/connection-loss gates, post-settlement row/lock/worker assertions, unchanged sequence on rejection. |
| CA-09 | This ADR — Normative Contract Clauses | Preserve other boundaries and terminal append; safe typed diagnostics. | AC-2, AC-6 | Foreground append regression, routed diff inspection, captured diagnostics with sentinel content. |

## Risk Coverage Matrix [Conditionally Required — source or configuration implementation]

| Risk dimension | Applicability and scenario, or specific N/A reason | Owning boundary | Deterministic verification method | Exact expected result | Acceptance check IDs | Status | Actual evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Concurrency and ordering | Applicable — 32 writers target one tip; retries overlap; independent chains share a Turn. | C-6 PostgreSQL correction transaction | Real PostgreSQL barriers under the measured AC-3 timing precondition; compare result vectors, sequence counter, original rows, and raw replay; AC-4 separately forces deadline exhaustion. | With AC-3 timing satisfied, one winner/31 tip conflicts and identical requests return the same Item; independent chains commit distinct sequences. Deadline-exhausted calls follow CA-07 without duplicate successors. | AC-3, AC-4 | Pass | AC-3 at `849b0c2`: one winner plus 31 `PredecessorConflict` under the measured bound, identical calls converged on one Item, distinct increasing sequences per chain; AC-4's 32 deadline-exhausted writers kept at most one durable successor and deduplicated on retry. |
| Timeout and deadline | Applicable — pool, operation lock, Turn lock, query, or commit/reconciliation stalls. | C-6 correction settlement and SQLx | Fake-time budget test plus real PostgreSQL stalled-lock and lost-acknowledgement harness. | Each attempt expires at 2 seconds; at most one write and one reconcile; success only from durable exact match, unresolved state is Unavailable. | AC-4 | Pass | `correction_settlement_budget` proved the exact 2-second/4-second arithmetic with paused time (7/7); the real identity-lock stall consumed both budgets into `Unavailable` and the 32-writer deadline-exhaustion case stayed bounded and unique. |
| Cancellation and interruption | Applicable — cancellation before commit or connection loss racing commit. | C-6 SQLx transaction ownership | Controlled precommit cancellation, commit acknowledgement loss, lock-release checks and later exact retry. | Precommit abort changes no row/counter; committed result survives; no detached write or retry worker; original terminal unchanged. | AC-4, AC-5 | Pass | AC-4: the backend terminated mid-attempt rolled back with zero mutation and settled into `NotApplied`; every controlled trigger fault in AC-5 rolled back to byte-identical snapshots; no detached retry worker exists by construction (the settlement owns no spawned tasks). |
| Resource bounds and backpressure | Applicable — maximal content, ancestor count, payload size, or contended pool. | Command validation and bounded C-6 reads | Limit/one-over fixtures, query result-size instrumentation and unavailable pool test. | 65,536-byte command and 4,096 ancestors accepted; one-over rejected; oversized stored payload not fetched; pool wait consumes the same attempt budget. | AC-1, AC-4, AC-5 | Pass | AC-5 at `849b0c2`: 4,095/4,096-node chains and 1,048,576-byte stored payloads admitted inclusively, one-over rejected as `ResourceLimit` on both ancestor and retry reads without fetching the body; the contended-pool case consumed the same attempt budget in AC-4's 32-writer case. |
| Framework or trust-boundary rejection | Applicable — wrong owner, malformed stored data, invalid terminal/kind, SQL constraint conflict. | Owned command and production PostgreSQL adapter | Real PostgreSQL ownership/state/kind/corruption and constraint cases. | Exact typed failures, zero new durable mutation, no foreign data disclosure, constraints remain enforced. | AC-2, AC-3, AC-5 | Pass | AC-2 at `849b0c2`: every ownership, kind, ancestry, counter, and stored-identity case returned its exact typed category with zero mutation; the production foreign key and check constraints were separately proven to reject the fixture-only corrupt rows; AC-5's trigger faults stayed typed and constraints remained enforced. |

## Acceptance Checks [Required]

T-1/T-2 must introduce the exact test targets below. A zero-test execution is
failure. PostgreSQL acceptance requires `KODUCK_AI_TEST_DATABASE_URL` pointing
to an isolated test database with all existing production migrations applied;
the new PostgreSQL test binary must fail explicitly when that environment is
missing. This is an intentional prerequisite for full routed/acceptance runs,
including AC-6 `cargo test -p koduck-ai --all-targets --all-features`: export
the URL into the invoking process so every child test receives it. CI must
also supply the isolated database. Without it, failure is expected and AC-6
cannot be reported Pass; no new opt-in switch or default skip is introduced.
Database-free local iteration may run AC-1
(`cargo test -p koduck-ai --test cand_11_correction_admission command_validation -- --exact`)
or another selected database-free test target; that is partial verification,
not AC-6 evidence. Never print database credentials. Corrupt-row cases that normal constraints
prevent must use an isolated fixture schema/database, restore any fixture-only
constraint changes, and separately prove the unmodified production constraints
reject those rows. A corrupt fixture is never a proposed migration or a
production bypass. Private async fault seams may be tested
in a colocated module, but each declared PostgreSQL integration test must
still exercise the production `SqlxPostgresExecutor` correction port. AC-1 is
a command-only semantic check in an integration-test binary; it requires zero
store calls and does not require a database. Test-created database
state and compiler output are disposable and must be cleaned after verification.

AC-3 isolates arbitration from deadline exhaustion using shallow valid chains,
a prewarmed pool with at least 32 available connections, and no unrelated
workload or injected database stall. The timing precondition is measured,
not inferred from an assumption that database writes take milliseconds. For
each 32-call case, record a conservative serialized bound
`W + 31 * L + S < 2,000 ms`: `W` bounds the winner's lock-held transaction and
completion, `L` is the maximum remaining caller's lock-held validation or
exact-retry lookup and completion, and `S` bounds the accumulated scheduling,
pool-acquisition, and lock-handoff delays not included in those intervals.
Also record every call's actual write-attempt elapsed time and verify that
none reaches its 2-second deadline. These measurements must include both the
identity-lock queue and Turn-lock queue; they cannot omit pool or wakeup costs.

If that precondition cannot be established, the test exits nonzero with an
explicit timing-precondition diagnostic and AC-3 remains unproven; do not
interpret a legitimate CA-07 outcome as a broken single-winner rule, accept
that run as Pass, relax production deadlines, or retry until a green run appears.
AC-4 deliberately exceeds the budget and checks its exact settlement outcomes
and uniqueness independently. Thus AC-3 proves the conditional arbitration
result and AC-4 proves timeout behavior; neither claims a universal latency
guarantee for arbitrary CI machines.

| Check ID | Subtask | Binary acceptance point | Preconditions or input | Verification method | Exact expected result | Expected evidence | Status | Actual result and evidence |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| AC-1 | T-1 | CA-01 command validation is exact. | Empty/whitespace, one byte, 65,535/65,536/65,537 UTF-8 bytes, multibyte text and self identity. | `cargo test -p koduck-ai --test cand_11_correction_admission command_validation -- --exact` | Exit 0; one test runs; nonblank <=65,536-byte content preserved exactly, invalid input has exact category with zero store calls; caller cannot set sequence or discriminator. | Command output and semantic fixture matrix; compiler/API inspection for private fields. | Pass | On 2026-09-05 at tested revision `849b0c2` the command exited 0 with `1 passed`, covering blank/whitespace, 1-byte, 65,535/65,536/65,537-byte, multibyte, and self-identity cases with the exact typed categories; the command surface is read-only with private fields. |
| AC-2 | T-2 | CA-02/CA-03 admission and CA-05/CA-09 preservation hold. | Real PostgreSQL; six Turn states, matching/wrong tenant/subject/Thread/Turn, every Item kind, root and repeated-chain tips, stale tips, missing/foreign/cyclic/branched/nondecreasing/malformed ancestors, invalid next_sequence; exact and mismatched existing identities on each nonterminal fixture; terminal-Turn stored correction rows matching the tenant/Thread/Turn/Item/predecessor lookup keys with below-cap invalid JSON or a missing/non-string content member, seeded in the isolated corrupt-data fixture. | `cargo test -p koduck-ai --test postgres_cand_11 admission_matrix -- --exact` | Exit 0; one test runs; four terminal states accept valid tips; fresh identities on two nonterminal states return TurnNotTerminal; existing exact identities on either nonterminal state return CorruptHistory and mismatched identities return IdentityConflict; each malformed stored retry payload returns CorruptHistory before content equality can be evaluated, with no row/counter change; every invalid case has its declared error and no new mutation; admitted Item uses the next sequence; original rows, terminal, flags and leases are equal; ordinary append still returns AlreadyTerminal. | Named case results, before/after row snapshots and raw-replay equality excluding the one new Item. | Pass | On 2026-09-05 at tested revision `849b0c2` the test passed in 0.5s: all four terminal states admit with lifecycle fields unchanged, `started`/`recovery-pending` reject fresh writes, every ownership drift is an indistinguishable `NotFound`, all Item kinds classify exactly, cyclic/forward-linked/malformed/broken/branched shapes fail closed (broken links and branches inside the isolated constraint-free fixture schema, with the production foreign key separately proven to reject the row), stale/overflowing counters are `CorruptHistory`, stored identities resolve exactly, and the foreground append regression still returns `AlreadyTerminal`. |
| AC-3 | T-2 | CA-04/CA-05 concurrency and retry converge. | Real PostgreSQL with 32 barrier-started calls satisfying the measured timing precondition above; fresh identities at one tip, 32 identical calls, independent chains, exact retry after later successor, each identity-bound field changed individually, reused ID in another tenant. | `cargo test -p koduck-ai --test postgres_cand_11 concurrency_and_retry -- --exact` | Exit 0; one test runs; timing precondition proven without deadline exhaustion; fresh same-tip calls produce one Item and 31 PredecessorConflict outcomes; identical calls all return one identical Item; independent chains increment distinct sequences once each; exact retry never writes; drift returns IdentityConflict; separate tenants do not collide. | Measured W/L/S bounds and per-call elapsed times, barrier/result vector, durable count and counter delta, full returned Item equality, original-row snapshots. | Pass | On 2026-09-05 at tested revision `849b0c2` the test passed in 0.53s: the conservative serialized bound was measured from three uncontended samples (W = L = measured maximum single-call latency, 32 x base well under the 2,000 ms budget), all 32-writer cases kept every per-call elapsed time under the 2-second deadline, exactly one winner plus 31 `PredecessorConflict` outcomes were observed with a single counter advance, 32 identical calls converged on one equal durable Item, independent chains received distinct increasing sequences, the exact retry after a later successor returned the original with zero new rows, and cross-tenant identity reuse admitted independently. |
| AC-4 | T-2 | CA-07/CA-08 settlement is bounded and truthful. | Real PostgreSQL stalls at pool/identity/Turn locks, including a deliberately deadline-exhausted 32-writer case; controlled commit-ack loss with commit or rollback, blocked reconciliation, wrong-owner lookup, caller/attempt cancellation; fake-clock tests for exact 2-second budgets. | `cargo test -p koduck-ai --test postgres_cand_11 settlement_and_cancellation -- --exact` plus `cargo test -p koduck-ai correction_settlement_budget` for the colocated deterministic budget tests. | Each command exits 0 and runs its named tests; <=1 write and <=1 reconciliation, 2-second budget each; reconciler cannot declare absence while writer holds identity lock; committed exact match succeeds, proven absence is NotApplied, unresolved state is Unavailable; deadline-exhausted concurrency permits CA-07 outcomes while retaining at most one durable successor and no duplicate on exact retry; cancellation before commit rolls back; later exact retry cannot duplicate; all locks/owned futures settle without a worker leak. | Fake-time timeline, real lock/commit traces, durable snapshots, nonzero test counts, typed outcomes and cleanup results. | Pass | On 2026-09-05 at tested revision `849b0c2` the harness passed in 9.4s and the budget unit tests passed 7/7 with paused Tokio time proving the exact 2-second write and reconciliation budgets and single-reconciliation bound. Real-boundary outcomes: an identity-lock stall consumed both budgets into `Unavailable` with zero mutation and a clean later admission; a Turn-row stall terminated mid-attempt settled into `NotApplied` with zero mutation; a committed exact match was observed by one reconciliation during a stall without duplicates; a wrong-owner retry was an indistinguishable `NotFound`; 32 deadline-exhausted writers all reported `Unavailable` with no sequence allocation and clean deduplicated admission after release. The fake-time timeline, typed outcomes, and per-case snapshots are the recorded evidence. |
| AC-5 | T-2 | CA-05/CA-06/CA-08 enforce bounds and zero mutation. | Real PostgreSQL; valid chains with existing node counts of 4,095/4,096/4,097, including both the predecessor and root (a root-only target counts as one); canonical payloads of 1,048,575/1,048,576/1,048,577 stored bytes on both ancestor and retry reads; BIGINT overflow/nonpositive/stale sequence fixtures; controlled insert/counter-update failures and precommit cancellation. | `cargo test -p koduck-ai --test postgres_cand_11 bounds_and_atomicity -- --exact` | Exit 0; one test runs; count and stored-payload limits inclusive, one-over ResourceLimit; oversized payload body never fetched; sequence corruption is CorruptHistory; every proven rejection/rollback preserves all preexisting rows and next_sequence; no partial insert or counter update. | Query row/byte instrumentation, fixture outcomes, raw row/counter snapshots and rollback evidence. | Pass | On 2026-09-05 at tested revision `849b0c2` the test passed in 3.6s: 4,095- and 4,096-node chains admitted with exactly one counter advance, the 4,097th node was a `ResourceLimit` with zero mutation, the 1,048,576-byte stored payload was inclusive and one byte over was a `ResourceLimit` on both ancestor and retry reads (the oversized retry body carries invalid JSON, proving it was never decoded), stale and overflowing counters failed closed as `CorruptHistory`, the production check proved a nonpositive counter cannot exist, and the tenant-scoped controlled insert and counter-update trigger faults both rolled back to byte-identical snapshots with `Unavailable` outcomes and clean later admissions; the trigger and schema changes were restored inside the test. |
| AC-6 | T-2 | CA-06/CA-09 scope, diagnostics, and routed quality gates hold. | Implementation complete; explicit base and tested revisions; KODUCK_AI_TEST_DATABASE_URL exported to the full routed process and pointing to the isolated migrated PostgreSQL database; sentinel content in rejected requests. Missing URL intentionally fails the PostgreSQL binary and leaves this acceptance prerequisite unsatisfied. | Run `cargo fmt --all --check`; `cargo clippy -p koduck-ai --all-targets --all-features -- -D warnings`; `cargo test -p koduck-ai --all-targets --all-features`; `npm test --prefix tools/governance-validator`; `npm run validate --prefix tools/governance-validator`; inspect the affected revision diff and captured diagnostics. | With the database prerequisite satisfied, all commands exit 0; database tests actually run; no source/config/dependency change outside the declared boundary, full-history allocation, route/event/provider/lease/Tool behavior change, or sensitive sentinel/credential/raw SQL value in diagnostics; affected units satisfy the engineering limits. | Exact revisions and command reports; clause-linked diff/diagnostic/size review; matching required CI results and latest-revision automatic-review disposition before review-ready. | Pass | On 2026-09-05 at base `8e432bd` and tested implementation revision `849b0c2` with `KODUCK_AI_TEST_DATABASE_URL` exported to the invoking process: `cargo fmt --all --check` exited 0, `cargo clippy -p koduck-ai --all-targets --all-features -- -D warnings` exited 0, the full `cargo test -p koduck-ai --all-targets --all-features` run exited 0 with every suite green including the database-backed PostgreSQL harnesses (the missing-URL failure path is asserted by construction), and both governance validator commands exited 0 (184 passed/0 failed; validation passed). The diff review found no change outside the declared boundary (application port module, PostgreSQL correction child module and its budget-test child module, module declarations, test harnesses, and the dev-only tokio `test-util` feature for the ADR-declared fake-time tests; no new dependency, no version change, Cargo.lock unchanged), no route/event/provider/lease/Tool behavior change, and no content, credential, or raw SQL parameter in diagnostics — errors are typed categories only. Affected executable units were measured against the 80-line hard limit and the long ones decomposed; largest units are `validate_ancestry` (split into fetch, summary checks, and predecessor decode) and the test scenario functions, each now within the limit. |

Allowed final check statuses are `Pass`, `Fail`, or `N/A — <specific reason>`.
`Fail` blocks completion. Risk and acceptance results remain Not Started until
implementation exists and the declared production-boundary checks run.

## Completion Checklist [Required]

| ID | Item | Completion Criterion | Expected Evidence | Status | Actual Evidence |
| --- | --- | --- | --- | --- | --- |
| A-1 | ADR approved | Eligible non-author @linhai approves this specific ADR and approval metadata is complete. | ADR metadata and task approval context. | Complete | Required approver `@linhai` (non-author; Author is `@codex`) approved this specific ADR at `2026-09-05T07:51:32+08:00`: the repository owner issued the explicit `Approve` instruction citing this exact repository-relative path, then confirmed `@linhai Approve` in the same approval context, matching the ADD-0001 reapproval identity protocol. Approval metadata is complete. ADD approval does not accept this ADR; this approval is ADR-specific. |
| A-2 | Complete task delivered | T-1/T-2 complete and AC-1 through AC-6 Pass. | Source revision, test reports and implementation evidence. | Complete | T-1/T-2 are Complete in commit `849b0c2ffa722c70484e1516ab1e122ac3f8ca9c` on branch `codex/cand-11-correction-admission` (base `8e432bd`); AC-1 through AC-6 are all `Pass` on 2026-09-05. |
| A-3 | Reciprocal ADD link synchronized | CAND-11 names this ADR and is Selected while it is Proposed/Not Started; it becomes Complete only with this ADR Complete/Verified. | ADD candidate, ADR Architecture Source and central indexes. | Complete | Current-stage reciprocal path and Selected/Proposed/Not Started agreement confirmed by governance validation on 2026-09-04. Candidate completion is still pending this ADR reaching Complete/Verified; this row does not certify that future transition. |
| A-4 | Requirement levels satisfied | Every required field complete for its lifecycle; every conditional trigger assessed. | Structured draft review. | Complete | Structured review round 1 on 2026-09-04: all required draft-stage content present, conditional triggers assessed, precise check inputs/outcomes and no implementation evidence claimed. |
| A-5 | Acceptance checks are decidable | Every check specifies inputs, exact result, method, subtask and evidence. | Structured check review. | Complete | Structured review round 1 on 2026-09-04: all required draft-stage content present, conditional triggers assessed, precise check inputs/outcomes and no implementation evidence claimed. |
| A-6 | Engineering exceptions governed | No waived rule; any discovered exception is approved before retaining it. | Engineering Exceptions assessment and implementation size review. | Complete | On 2026-09-05 at `849b0c2`: no rule is waived and the Engineering Exceptions section remains `N/A`. Every affected executable unit was measured against the 80-line hard limit: `validate_ancestry` was decomposed into fetch (`validate_ancestry`), summary checks (`reject_invalid_summary`), and predecessor decode (`reject_malformed_predecessor`); the long test scenario functions were decomposed into per-case functions, all now within the limit. Two 60-80-line units (`correct_async`, `stored_retry`) are reviewed-cohesive: each is one admission transaction/identity-resolution flow whose splitting would scatter its error precedence. clippy `too_many_lines` (100-line) passes for every unit. |
| A-7 | Contract and baseline risks covered | CA-01 through CA-09 mapped; five risk dimensions specified, each eventually Pass. | Traceability and Risk Coverage Matrix. | Complete | CA-01 through CA-09 map to AC-1 through AC-6 (all `Pass` on 2026-09-05 at `849b0c2`), and all five Risk Coverage Matrix rows are `Pass` with recorded evidence. |
| A-8 | Governance validation passed | Both routed governance commands exit 0 for final document state. | Validator test and repository validation reports. | Complete | On 2026-09-04, `npm test --prefix tools/governance-validator` exited 0 (184 passed, 0 failed); `npm run validate --prefix tools/governance-validator` exited 0 after the human-review revisions as well as the earlier draft/link changes. Both governance commands were rerun for these revisions (184 passed, 0 failed), and `git diff --check` passed. On 2026-09-05, both commands were rerun for the final accepted state including the acceptance metadata and Change Log entry: `npm test` exited 0 (184 passed, 0 failed), `npm run validate` exited 0, and `git diff --check` passed. |

## Supporting Notes [Optional]

Drafting is documentation-only. No source implementation, regression test,
dependency, migration, build artifact, or deployment is created by this change.
The drafting task is `01a06ae7-ba41-7562-b618-f6b85021cfdd`, on branch
`codex/cand-11-correction-admission` from local `dev` at
`8e432bd60b56f754f188eda57a60866374b8e8e2`. The pre-existing AGENTS.md edits are
outside this change. Trello's linked requirement was checked read-only on
2026-09-04 and remains the recorded AI-service boundary/Codex-alignment demand;
no Trello mutation was authorized or performed.

Structured review round 1 inspected the new Proposed ADR and the status/link
diff against local base `8e432bd60b56f754f188eda57a60866374b8e8e2` on
2026-09-04. It clarified error precedence, fixture-only corruption setup, and
fully qualified implementation anchors; these corrections were incorporated
before requesting ADR approval. Follow-up structured review round 2 checked
those corrections, all nine clause mappings, the five risk dimensions, lifecycle
metadata, candidate scope, and reciprocal indexes; no remaining actionable
draft finding was identified. Review targets were the local document changes
against that base, not a pushed implementation commit. The later human review
and its requested clarifications are recorded below; the earlier review result
is historical and does not cover those subsequent edits. No additional
agent-driven review round was launched for this targeted remediation.
No remote review was requested and no revision was pushed; this draft does
not claim automatic-review coverage or implementation review readiness.

The current `koduck-ai` Scope Routing row has no canonical local SonarQube
scanner workflow; feature completion therefore uses the routed checks under
ADR-0015's applicability rule. Do not invent scanner parameters or represent
this as a SonarQube pass. A future implementation revision must recheck routing
and satisfy its applicable CI, automatic-review, and completion gates.

Implementation note (2026-09-05, revision `849b0c2`): the acceptance
implementation lives in `koduck-ai/src/application/correction_store.rs`,
`koduck-ai/src/adapters/history/postgres/sqlx_executor/correction.rs`, and its
`correction_settlement_budget` child module, with the AC harnesses under
`koduck-ai/tests/`. Three implementation decisions are recorded here as
evidence, none of which widens the accepted scope: (1) the dev-only tokio
`test-util` feature was enabled for the AC-4 fake-time budget tests — a
feature of the already-locked tokio dependency, not a new dependency and not a
version change (Cargo.lock unchanged); (2) the ancestor summary is computed
server-side in one bounded recursive query because the measured generic-plan
degeneration of the parameterized recursive join exceeded the CA-07 budget,
so the statement is executed uncached (`.persistent(false)`) and the LATERAL
probe keeps every plan shape at one index probe per node; and (3) the
correction INSERT is stated in the correction module, classified through
`WriteFailure`, because the shared `insert_item` helper flattens every failure
to `HistoryError::Unavailable` and would hide the statement-rejected versus
transport-lost distinction CA-07 reconciles on — the durable columns still
come from the shared `DurableItemCodec` and the production constraints are
unchanged.

## Archival [Conditionally Required — Decision Status is `Rejected`, or Decision Status is `Deprecated` or `Superseded` and Implementation Status is final]

Inactive future-lifecycle guidance while this ADR is Proposed. When triggered,
move this file to `archive/ADR-0004-authenticated-correction-admission.md` in the
same service ADR root; update all code markers, reciprocal ADD links and index
paths atomically. Supersession additionally requires reciprocal replacement
paths; otherwise retain `Superseded By: None`. No live reference may retain the
pre-archive path.

## Change Log [Required]

| Date | Change | Author |
| --- | --- | --- |
| 2026-09-04 | Proposed the CAND-11 internal correction-admission decision after ADD-0001 reapproval by @linhai. Selected exactly CAND-11 with reciprocal paths and added the central index row. No implementation or ADR approval is implied. | @codex |
| 2026-09-04 | Structured review round 1 clarified deterministic error precedence and isolated corruption-fixture setup, and expanded implementation anchors to fully qualified symbols. Draft remains Proposed/Not Started; acceptance and risk checks remain unexecuted. | @codex |
| 2026-09-04 | Follow-up structured review round 2 found no remaining actionable draft finding. Governance tests passed (184/184), repository validation passed, and whitespace checks passed. Source/SQLx acceptance checks were not run because this task creates documentation only and those implementations/test targets do not yet exist. | @codex |
| 2026-09-04 | Addressed the human review in task `01a06ae7-ba41-7562-b618-f6b85021cfdd`: clarified content versus stored-read limits, current next_sequence, nonterminal Item flag, identity-versus-state precedence and corruption outcome, and measured AC-3 timing prerequisites with separate AC-4 deadline-exhaustion coverage. Aligned Pros/Cons formatting, wrapped the transaction paragraph, added CA-04 read-cap navigation, and scoped A-3 completion evidence to the current lifecycle. Updated linked acceptance/traceability/risk cells. These are requested Proposed-record revisions, not ADR approval or a new agent-driven review round. Verification: governance tests 184 passed/0 failed; repository validation and `git diff --check` passed. Source acceptance checks remain unexecuted because no source implementation was requested. | @codex |
| 2026-09-04 | Addressed the second human review in task `01a06ae7-ba41-7562-b618-f6b85021cfdd`: made the database URL mandatory for full routed/AC-6 verification and documented database-free focused iteration; added AC-2 malformed stored-retry payload fixtures and CA-04 traceability; identified the advisory key as correction ItemId-derived; clarified that the private ancestor settlement helper remains descendant-accessible but has incompatible HistoryError typing; included CAND-3 CR-08 recovery, unified node-count terminology, limited the production-port requirement to PostgreSQL tests, documented pub(super) reuse, and renamed the planned application module to correction_store.rs. This is targeted remediation of human findings, not another agent-driven review round. Proposed/Not Started and ADD links remain unchanged. Verification after these revisions: governance tests 184 passed/0 failed; repository validation and `git diff --check` passed. No source or PostgreSQL acceptance tests ran because their implementation is still Not Started. | @codex |
| 2026-09-05 | Recorded acceptance by the required approver: the repository owner issued the explicit `Approve` instruction citing this exact repository-relative path, then confirmed `@linhai Approve` in the same approval context, following the ADD-0001 reapproval identity protocol. Decision Status is `Accepted`; Implementation Status remains `Not Started`. No Approval Context Revision is recorded because approval precedes the first immutable commit containing this content. Acceptance checks AC-1 through AC-6 and the Risk Coverage Matrix remain unexecuted pending implementation; subtask statuses are unchanged. | @zcode |
| 2026-09-05 | Implemented CAND-11 T-1/T-2 in commit `849b0c2ffa722c70484e1516ab1e122ac3f8ca9c` on branch `codex/cand-11-correction-admission` (base `8e432bd`): the owned application port, the guarded PostgreSQL admission transaction with bounded server-side ancestry validation and the two-budget settlement, and the AC-1 through AC-5 harnesses. All acceptance checks and all five Risk Coverage Matrix rows are `Pass`; routed checks (fmt, clippy, full test suite with the isolated database, both governance validator commands) exited 0. Implementation Status is `Verified`. The dev-only tokio `test-util` feature, the uncached server-side ancestry summary, and the classified in-module INSERT are recorded in Supporting Notes. | @zcode |
