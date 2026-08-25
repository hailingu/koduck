# ADR-0003: Correction Item Schema and Raw Replay

## Metadata [Required]

- **Decision Status**: Accepted
- **Implementation Status**: Complete
- **Date**: 2026-08-25
- **Author**: @codex
- **Decision Owner**: @linhai
- **Required Approver**: @linhai
- **Record Scope**: Service internal — koduck-ai
- **Approver [Conditionally Required — Decision Status is or has been `Accepted`]**: @linhai
- **Approval Time [Conditionally Required — Decision Status is or has been `Accepted`]**: 2026-08-25T22:43:30+08:00
- **Approval Evidence [Conditionally Required — Decision Status is or has been `Accepted`]**: Approve
- **Approval Context Revision [Optional — informational and non-binding]**: `88a8e4f` — the first immutable revision containing the approved document content; informational only, not approval evidence.
- **Rejector [Conditionally Required — Decision Status is `Rejected`]**: N/A — Decision Status is `Accepted`
- **Rejection Time [Conditionally Required — Decision Status is `Rejected`]**: N/A — Decision Status is `Accepted`
- **Rejection Evidence [Conditionally Required — Decision Status is `Rejected`]**: N/A — Decision Status is `Accepted`
- **Retired By [Conditionally Required — Decision Status is `Deprecated` or `Superseded`]**: N/A — Decision Status is `Accepted`
- **Retirement Time [Conditionally Required — Decision Status is `Deprecated` or `Superseded`]**: N/A — Decision Status is `Accepted`
- **Retirement Evidence [Conditionally Required — Decision Status is `Deprecated` or `Superseded`]**: N/A — Decision Status is `Accepted`
- **Retirement Reason [Conditionally Required — Decision Status is `Deprecated` or `Superseded`]**: N/A — Decision Status is `Accepted`
- **Blocked From [Conditionally Required — Implementation Status is `Blocked`]**: N/A — Implementation Status is `Complete`
- **Blocker And Evidence [Conditionally Required — Implementation Status is `Blocked`]**: N/A — Implementation Status is `Complete`
- **Blocker Owner [Conditionally Required — Implementation Status is `Blocked`]**: N/A — Implementation Status is `Complete`
- **Blocker Exit Or Recheck Criterion [Conditionally Required — Implementation Status is `Blocked`]**: N/A — Implementation Status is `Complete`
- **Related [Optional]**: `docs/adr/ADR-0001-provider-neutral-turn-kernel.md`; `docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md`
- **Architecture Source [Conditionally Required — product demand]**: `docs/architecture/ADD-0001-ai-service-codex-alignment.md` — CAND-3
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
  implementation, completion, or verification. If retained, it MUST be
  accurate and complete; optional content MUST NOT substitute for required
  evidence.

Unlabeled fields inside a `[Required]` section are required.

## Context And Problem Statement [Required]

CAND-1 established immutable, durably sequenced Thread/Turn/Item history and a
PostgreSQL-backed `TurnHistory` replay boundary. The current Item model and
payload codec cannot represent that one later Item corrects one earlier Item.
Updating the earlier row would destroy durable evidence, while storing an
untyped second message would lose the relationship and make downstream meaning
ambiguous.

ADD-0001 CAND-3 now defines only the representation foundation: a typed
correction Item, an additive durable relationship, fail-closed decoding, and
raw replay that retains every original and correction Item. Authenticated write
admission and concurrency belong to CAND-11, effective projection belongs to
CAND-12, and provider-context integration belongs to CAND-13. This ADR must not
preempt those boundaries.

## Scope [Required]

In scope:

- Add one typed correction Item payload that carries replacement content and a
  predecessor Item identity.
- Evolve the PostgreSQL history schema additively to persist the relationship
  in the same tenant, Thread, and Turn and prevent self-reference or more than
  one direct successor.
- Extend the durable payload codec and row decoder for the correction type.
- Preserve raw replay as every original and correction Item exactly once in
  increasing sequence order.
- Fail closed when a stored correction relationship or payload cannot be
  decoded as the declared structure.
- Preserve all pre-migration CAND-1/CAND-2 rows, constraints, terminal outcomes,
  and domain replay results.

Out of scope:

- Any authenticated correction command or `TurnHistory` write operation.
- Tenant/subject/terminal/predecessor-kind admission policy, sequence
  allocation, stable-identity retry, concurrent single-winner arbitration, or
  ambiguous-acknowledgement reconciliation.
- Effective-context projection, chain-tip substitution, provider message
  construction, provider aggregate limits, semantic Memory projection, or
  Multitask integration.
- A northbound endpoint, REST/SSE event, UI, client routing, Thread fork,
  checkpoint, deployment, promotion, or rollback operation.
- Updating, deleting, compacting, hiding, or reordering an existing Item.

## Tensions, Constraints, And Open Questions [Required]

### Identified Tensions [Conditionally Required — competing goals or trade-offs exist]

| ID | Tension | Impact | Decision |
| --- | --- | --- | --- |
| TN-1 | The schema must make corrections explicit without authorizing the future write operation. | Pulling admission into this slice would recreate the oversized cross-boundary task. | Persist only the structural relationship and typed payload; CAND-11 owns all authenticated transactional admission. |
| TN-2 | Raw replay must expose complete evidence while later consumers will need corrected meaning. | Applying substitution during replay would hide original rows and couple persistence to provider semantics. | Raw replay returns every Item unchanged and ordered; CAND-12 owns a separate pure effective projection. |
| TN-3 | Adding an exhaustive Item variant affects adjacent match sites. | Treating compile-only arms as provider integration would widen the primary boundary. | Adjacent exhaustive matches may receive only the minimum non-publishing/non-integrating compatibility arm required to compile; no consumer gains correction semantics in this ADR. |

### Constraints [Required]

- `docs/adr/ADR-0001-provider-neutral-turn-kernel.md` remains authoritative for
  stable Item identity, positive Turn-local sequence, ordered replay, immutable
  history, one terminal outcome, migration deadlines, and AI ownership.
- `docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md` remains
  authoritative for D-3/D-6/D-7 projection and audit Items; correction schema
  work must not change their authority, payload, or constraints.
- The primary implementation boundary is C-6 persistence/data behavior inside
  `koduck-ai`. Domain type declarations, runtime migration registration, and
  exhaustive-match compatibility are limited supporting changes.
- Existing migrations 0001 through 0008 are immutable. The new migration is
  forward-only and idempotent.
- The schema may enforce structural relationship invariants but must not invent
  CAND-11 admission outcomes or CAND-12/CAND-13 consumer semantics.
- No dependency, public route, external contract, or deployment configuration
  may be added by this task.

### Open Questions [Conditionally Required — material questions exist or were resolved during drafting]

None — representation ownership, raw replay behavior, structural constraints,
migration compatibility, and the boundaries deferred to CAND-11 through
CAND-13 are resolved by this decision.

## Decision Drivers [Required]

1. **Immutable evidence**: Existing Items and replay positions remain unchanged.
2. **Typed durability**: A correction relationship has one owned domain and
   storage representation rather than an inferred convention.
3. **Narrow reviewability**: Schema, codec, migration, and raw replay remain one
   persistence/data slice with no admission or provider policy.
4. **Fail-closed compatibility**: Invalid stored correction data is rejected,
   never guessed or silently rewritten.
5. **Forward compatibility**: Later candidates can build on the representation
   without changing existing CAND-1/CAND-2 history.

## Options Considered [Required]

### Option: Update the original Item in place

Pros:

- No new Item kind or relationship.

Cons:

- Destroys original evidence and invalidates replay identity.
- Makes correction time and provenance unrecoverable.

### Option: Store an untyped second message

Pros:

- Reuses the existing payload codec unchanged.

Cons:

- Loses the predecessor relationship.
- Makes later projection infer meaning from order or prose.
- Cannot structurally reject multiple direct successors.

### Option: Add a typed correction relationship and preserve raw replay

Pros:

- Preserves every original row and makes the relationship explicit.
- Gives CAND-11 through CAND-13 one stable input contract.
- Keeps this slice within the persistence/data boundary.

Cons:

- Adds a new Item variant and additive schema objects.
- Exhaustive Item matches need limited compatibility updates.
- The representation is intentionally not user-visible until later candidates.

## Decision [Required]

**Selected option**: Add a typed correction relationship and preserve raw
replay.

**Rationale**: This is the smallest independently mergeable foundation that
preserves immutable evidence and removes representation ambiguity. It creates
no write authority and no corrected provider behavior, so the transaction,
projection, and provider boundaries remain separately reviewable.

### Normative Contract Clauses [Required]

- **CR-01 — Typed representation**: The domain and durable codec represent a
  correction as one distinct Item kind containing replacement content and one
  `corrects_item_id`; ordinary message, usage, terminal, approval, Tool-call,
  and Tool-result Items retain their existing representations.
- **CR-02 — Durable relationship scope**: A persisted correction relationship
  identifies an existing Item in the same tenant, Thread, and Turn, and the
  correcting Item must not identify itself.
- **CR-03 — One direct successor**: The durable schema permits at most one
  direct correction successor for one predecessor. This structural uniqueness
  does not define the future caller-facing conflict or retry outcome.
- **CR-04 — Immutable ordered raw replay**: Replay returns every original and
  correction Item exactly once in increasing Turn-local sequence order and
  never updates, deletes, substitutes, hides, or resequences an Item.
- **CR-05 — Fail-closed structural rejection**: Unknown correction
  discriminators and missing or malformed correction fields return the existing
  typed unavailable/corrupt history outcome during decoding. Same-scope and
  non-self relationship violations are rejected by durable schema constraints;
  if malformed legacy or externally inserted relationship data reaches replay,
  replay fails closed. Neither path guesses a target, drops a row, or rewrites
  history.
- **CR-06 — Additive compatibility**: The forward migration is idempotent,
  does not edit migrations 0001 through 0008, retains every existing row and
  constraint, and leaves pre-migration domain replay equivalent after upgrade.
- **CR-07 — No consumer integration**: This task adds no correction admission,
  effective projection, provider serialization, REST/SSE event, or Memory
  delivery. Any adjacent exhaustive-match edit is limited to explicit
  non-publishing/non-integrating compatibility.
- **CR-08 — Recovery**: If the implementation is withdrawn before promotion,
  correction admission remains unavailable and all durable rows are retained;
  recovery uses only a verified reader/schema pair and never reverses a
  migration by deleting correction data.

### Consequences [Required]

Positive:

- Canonical history can carry an explicit correction relationship without
  sacrificing raw evidence.
- Migration and replay compatibility are independently testable.
- CAND-11, CAND-12, and CAND-13 receive a stable, narrow dependency.

Negative:

- The new Item kind is not yet useful to a user-facing caller.
- Exhaustive Item matches need explicit compatibility treatment.
- Correct admission and effective meaning require later candidates.

Mitigations:

- Keep correction admission unavailable in this slice.
- Verify old-row replay hashes and schema constraints against real PostgreSQL.
- Require later candidates to depend on this exact representation rather than
  re-defining it.

## Implementation Plan [Required]

**Complete task outcome**: One independently reviewable implementation pull
request adds the typed correction Item representation, additive PostgreSQL
schema and codec support, and immutable ordered raw replay while preserving all
existing history and adding no correction admission or consumer semantics.

**Primary implementation boundary**: Persistence and data behavior in the
`koduck-ai` C-6 canonical history adapter.

Allowed subtask statuses: `Not Started`, `In Progress`, `Blocked`, `Complete`,
or `N/A — <specific reason>`.

| ID | Objective or deliverable | Included scope | Status | Actual implementation evidence |
| --- | --- | --- | --- | --- |
| T-1 | Define the typed correction representation, codec behavior, and raw replay contract test-first. | Domain payload/type, payload encode/decode, structural replay validation, ordered raw replay fixtures, compile-only exhaustive-match compatibility, and `koduck-ai/docs/contracts/cand-3-correction-schema-v1.md`. | Complete | Red-first: `cargo test -p koduck-ai --test cand_3_correction_schema codec_and_compatibility -- --exact` failed with E0432/E0599 on the missing `ItemPayload::Correction`, `ItemCorrection`, `validate_raw_replay`, and codec symbols. Implemented in commit `c5211311e34bf`: `koduck_ai::domain::item_correction::{ItemCorrection, RawReplayStructureError, validate_raw_replay}`, the `ItemPayload::Correction` variant, `payload_codec/item_correction.rs`, the public `DurableItemCodec`/`DurableItemColumns` contract, and non-publishing/non-integrating arms in `provider_messages` and `item_created_event`; contract copy added at `koduck-ai/docs/contracts/cand-3-correction-schema-v1.md`. AC-1 and AC-2 pass. |
| T-2 | Add and verify the forward PostgreSQL schema evolution. | Migration 0009, same-scope/self/unique-successor constraints, runtime registration, idempotent upgrade, seeded CAND-1/CAND-2 compatibility, real-PostgreSQL tests, routed Rust checks, and final governance evidence. | Complete | Red-first: `cargo test -p koduck-ai --test postgres_cand_3 correction_schema_migration -- --exact` failed on the missing `migrations/0009_cand_3_correction_items.sql`. Implemented in commit `c5211311e34bf`: migration 0009 (relationship column, shape/self/same-Turn-scope constraints, one-direct-successor unique index, idempotent guards), registration in `apply_startup_migrations` and the shared test-migration list, and `replay_async` structural fail-closed validation. AC-3 passes against disposable `postgres:18-alpine`; AC-4 routed checks and governance validation exit 0. |

**Affected paths**: `koduck-ai/src/domain/mod.rs`,
`koduck-ai/src/domain/item_correction.rs`,
`koduck-ai/src/adapters/history/postgres/payload_codec.rs`,
`koduck-ai/src/adapters/history/postgres/payload_codec/item_correction.rs`,
`koduck-ai/src/adapters/history/postgres/sqlx_executor.rs`,
`koduck-ai/src/adapters/provider/messages.rs`,
`koduck-ai/src/adapters/http/wire.rs`, `koduck-ai/src/runtime/mod.rs`,
`koduck-ai/migrations/0009_cand_3_correction_items.sql`,
`koduck-ai/docs/contracts/cand-3-correction-schema-v1.md`,
`koduck-ai/tests/cand_3_correction_schema.rs`,
`koduck-ai/tests/postgres_cand_3.rs`, this ADR, the linked ADD candidate,
and `docs/adr/INDEX.md`.

### Stable Implementation Touchpoints [Conditionally Required — source or configuration implementation]

| Path | Stable symbol or contract anchor | Key code excerpt, when needed | Purpose | Source revision |
| --- | --- | --- | --- | --- |
| `koduck-ai/src/domain/mod.rs` | `koduck_ai::domain::ItemPayload`; `koduck_ai::domain::Item` | N/A — stable domain types own Item payload, identity, and sequence. | Add the typed correction representation without changing existing Item meanings. | `3d5043d1739a3a06222d7f96b37bfff0e4f3123f` |
| `koduck-ai/src/adapters/history/postgres/payload_codec.rs` | `encode_payload`; `decode_payload`; `row_to_item` | N/A — stable codec anchors own durable Item translation. | Encode/decode correction Items and fail closed on malformed durable data. | `3d5043d1739a3a06222d7f96b37bfff0e4f3123f` |
| `koduck-ai/src/adapters/history/postgres/sqlx_executor.rs` | `SqlxPostgresExecutor::replay_async` | N/A — the stable replay query orders all Turn Items by sequence. | Preserve complete ordered raw replay for originals and corrections. | `3d5043d1739a3a06222d7f96b37bfff0e4f3123f` |
| `koduck-ai/src/adapters/provider/messages.rs` | `provider_messages` | N/A — exhaustive payload matching is a supporting compatibility site only. | Compile with the new variant without implementing effective correction semantics. | `3d5043d1739a3a06222d7f96b37bfff0e4f3123f` |
| `koduck-ai/src/adapters/http/wire.rs` | `item_created_event`; `sync_body` | N/A — stable wire serializers enumerate or filter Item payloads. | Add no correction route or event while keeping exhaustive matches explicit. | `3d5043d1739a3a06222d7f96b37bfff0e4f3123f` |
| `koduck-ai/migrations/0001_cand_1_history.sql` | `turn_items`; `turn_items_one_terminal_per_turn`; `turn_items_thread_replay` | N/A — stable schema objects define the immutable history baseline. | Extend schema forward-only without modifying the baseline migration. | `3d5043d1739a3a06222d7f96b37bfff0e4f3123f` |
| `koduck-ai/src/runtime/mod.rs` | `apply_startup_migrations` | N/A — stable runtime anchor registers ordered embedded migrations. | Apply migration 0009 under the existing serialized startup deadline. | `3d5043d1739a3a06222d7f96b37bfff0e4f3123f` |

**Migration and rollback strategy [Conditionally Required — this replaces or
changes existing behavior]**: Add migration 0009 after the immutable 0001
through 0008 sequence. It adds the correction discriminator and relationship
columns/constraints without changing existing rows. Apply it idempotently under
the existing advisory-lock and startup-deadline behavior. Before future
admission is enabled, use only a reader/schema pair that understands the new
kind. On implementation failure, leave admission disabled and roll forward to
a verified compatible reader; never delete or rewrite committed correction
rows. Pre-migration databases remain valid inputs and old rows replay unchanged.

### Engineering Exceptions [Conditionally Required — an engineering rule is exceeded or waived]

N/A — no engineering rule is waived. Existing large files receive only narrow
type, delegation, registration, or exhaustive-match compatibility edits; new
codec behavior belongs in a cohesive child module.

## Contract-To-Check Traceability [Conditionally Required — source or configuration implementation]

| Clause ID | Authoritative contract path and heading | Exact normative requirement | Acceptance check or deterministic test IDs | Explicit coverage method |
| --- | --- | --- | --- | --- |
| CR-01 | This ADR — Normative Contract Clauses | Correction has one distinct typed domain and durable representation. | AC-1, AC-3 | Domain/codec round trips cover the new kind and every existing kind. |
| CR-02 | This ADR — Normative Contract Clauses | The durable target belongs to the same tenant, Thread, and Turn and is not self. | AC-2, AC-3 | Structural fixtures and real-PostgreSQL constraint cases cover every scope dimension. |
| CR-03 | This ADR — Normative Contract Clauses | One predecessor has at most one direct successor. | AC-2, AC-3 | Duplicate-successor fixtures assert deterministic structural rejection. |
| CR-04 | This ADR — Normative Contract Clauses | Raw replay returns every Item once in increasing sequence without mutation or substitution. | AC-2, AC-3 | Ordered fixtures and seeded replay hashes compare all identities, sequences, and payloads. |
| CR-05 | This ADR — Normative Contract Clauses | Malformed correction payloads fail during decoding; invalid scope/self relationships fail at schema enforcement or fail closed if malformed legacy/external data reaches replay; no row is guessed, dropped, or rewritten. | AC-1, AC-2, AC-3 | Codec/replay corruption matrices assert the stable typed failure, while real-PostgreSQL cases assert constraint rejection for scope and self violations. |
| CR-06 | This ADR — Normative Contract Clauses | Migration is additive, idempotent, and preserves old rows and constraints. | AC-3, AC-4 | Fresh PostgreSQL applies the full sequence twice and compares seeded CAND-1/CAND-2 data. |
| CR-07 | This ADR — Normative Contract Clauses | No admission, projection, provider correction semantics, route, or event is added. | AC-4 | The routed base-to-tested-commit diff proves correction remains non-admittable, non-projected, and non-published. |
| CR-08 | This ADR — Normative Contract Clauses | Recovery retains rows and uses a compatible reader/schema pair. | AC-3, AC-4 | Migration/replay evidence and governed diff confirm forward-only recovery. |

## Risk Coverage Matrix [Conditionally Required — source or configuration implementation]

| Risk dimension | Applicability and scenario, or specific N/A reason | Owning boundary | Deterministic verification method | Exact expected result | Acceptance check IDs | Status | Actual evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Concurrency and ordering | Applicable — concurrent startup may apply migration 0009 while replay ordering must remain deterministic. | Runtime migration registration and C-6 replay | Start two migration runners against one fresh PostgreSQL database, then replay mixed original/correction fixtures. | Both runners finish under the existing serialization contract; schema exists once; replay contains every row once in strictly increasing sequence. | AC-2, AC-3 | Pass | Commit `c5211311e34bf`: two concurrent advisory-lock-serialized runners and a second full application succeeded with each 0009 object existing exactly once, and mixed original-plus-correction replay returned 8 Items once in strictly increasing sequence. |
| Timeout and deadline | Applicable — migration 0009 runs inside the existing bounded startup sequence. | Runtime migration sequence | Inject a stalled migration operation under the existing startup deadline test. | Startup returns the existing database-timeout outcome within the approved bound and does not report a partially usable schema. | AC-3, AC-4 | Pass | Commit `c5211311e34bf`: a session-level holder of `STARTUP_MIGRATION_LOCK_KEY` stalled the advisory-lock startup sequence past its 500 ms deadline and the schema state was byte-identical before and after; the pre-existing `database_setup_attempt_maps_deadline_expiration` test stayed green in the routed suite. |
| Cancellation and interruption | N/A — this slice adds no write admission, background operation, lease, stream, or cancellable user action; startup transaction rollback is covered by migration failure tests. | Runtime startup migration transaction | Inspect affected paths and run migration rollback fixtures. | No cancellation API or state is added; a failed startup transaction exposes no partially registered correction schema. | AC-3, AC-4 | N/A — no new cancellable operation | Commit `c5211311e34bf`: routed diff adds no cancellation API or state; the AC-3 rollback leg proved a failed statement group persists neither the valid correction written before it nor any partial schema. |
| Resource bounds and backpressure | Applicable — malformed or oversized stored correction payloads must not create a second unbounded replay buffer. | Payload codec and C-6 replay | Run at-limit/over-limit durable payload fixtures under the existing Item/replay resource contract and inspect allocations introduced by the diff. | Existing bounds and failure outcomes remain unchanged; the correction decoder allocates only the owned payload and replay adds no second accumulated history. | AC-1, AC-4 | Pass | Commit `c5211311e34bf`: 1 MiB and 1 MiB+1 byte stored correction contents decode exactly once with no new replay-side byte bound; `validate_raw_replay` uses two bounded hash sets over the replayed slice and the routed diff adds no accumulated history buffer. |
| Framework or trust-boundary rejection | Applicable — durable rows may contain malformed discriminator, JSON, target identity, scope, or relationship shape. | PostgreSQL schema and payload decoder | Run the complete malformed-row matrix in codec tests and real PostgreSQL. | Every invalid case fails with the stable typed history error; no row is omitted, guessed, rewritten, or exposed through a new route/event. | AC-1, AC-2, AC-3 | Pass | Commit `c5211311e34bf`: the AC-1 matrix rejected every malformed discriminator/JSON/identity/relationship shape with `HistoryError::Unavailable`; AC-2 rejected every invalid replay structure with the stable `RawReplayStructureError`; and AC-3 proved real-PostgreSQL rejection by `turn_items_correction_not_self`, `turn_items_one_direct_correction`, `turn_items_correction_scope`, and `turn_items_correction_shape`. |

Allowed final statuses are `Pass`, `Fail`, or `N/A — <specific reason>`. `Fail`
blocks review-ready, `Complete`, and `Verified`.

## Acceptance Checks [Required]

| Check ID | Subtask | Binary acceptance point | Preconditions or input | Verification method | Exact expected result | Expected evidence | Status | Actual result and evidence |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| AC-1 | T-1 | Domain and codec behavior implements CR-01 and CR-05 without changing existing Item meanings. | Every existing Item kind; valid correction payload; missing, malformed, unknown, and oversized correction payload fixtures; adjacent exhaustive-match sites. | Run `cargo test -p koduck-ai --test cand_3_correction_schema codec_and_compatibility -- --exact`; T-1 must define `codec_and_compatibility` as that test binary's exact top-level test name. | Exit 0; exactly the named test runs; every existing kind round-trips unchanged; valid correction round-trips with exact content/target; every malformed case returns the stable typed error; all adjacent exhaustive-match compatibility sites compile. | Command output showing one exact test, exact case table, and compiler result. | Pass | Commits `c5211311e34bf` and `c9db8bdf64c2d`: command exited 0 with `1 passed; 1 filtered out` — exactly the named test ran; 15 existing-kind fixtures round-tripped unchanged; the valid correction round-tripped with exact content and target; 15 malformed fixtures (including the two extra-member shapes added by the PR-7 P2 review fix) plus per-kind relationship-column fixtures returned `HistoryError::Unavailable`; 1 MiB and 1 MiB+1 contents decoded exactly; the whole crate compiled, proving the provider/wire/runner compatibility arms. |
| AC-2 | T-1 | Raw replay and structural validation implement CR-02 through CR-05. | Mixed original/correction histories; same-scope and cross-scope targets; self-reference; duplicate direct successor; malformed relationship; non-contiguous fixture identities. | Run `cargo test -p koduck-ai --test cand_3_correction_schema raw_replay -- --exact`; T-1 must define `raw_replay` as that test binary's exact top-level test name. | Exit 0; exactly the named test runs; valid replay returns every Item once in increasing sequence with exact identities/payloads; every invalid structure fails closed and no Item is hidden, substituted, or reordered. | Command output showing one exact test, ordered fixture, and exact failure matrix. | Pass | Commit `c5211311e34bf`: command exited 0 with `1 passed; 1 filtered out`; the ordered mixed fixture with non-contiguous sequences 1/2/7/8/9 (two independent chains plus a correction-of-a-correction) validated unchanged; self-reference, absent cross-Turn target, duplicate direct successor, non-increasing sequence, and duplicate identity each failed with its exact `RawReplayStructureError` variant. |
| AC-3 | T-2 | PostgreSQL migration and replay implement CR-02 through CR-06 and CR-08. | Fresh disposable PostgreSQL seeded before migration with every CAND-1/CAND-2 Item kind and terminal state; valid and invalid correction relationship cases. | With `KODUCK_AI_TEST_DATABASE_URL=<fresh-disposable-postgresql-url>`, run `cargo test -p koduck-ai --test postgres_cand_3 correction_schema_migration -- --exact`; T-2 must define `correction_schema_migration` as that test binary's exact top-level test name. | Exit 0; exactly the named test runs; two concurrent migration runners and a second full application succeed; old rows/constraints/replay hashes are unchanged; valid corrections round-trip; self/cross-scope/duplicate-successor structures are rejected; failed migration exposes no partial schema. | Command output showing one exact test, database identity, schema assertions, and before/after replay hashes. | Pass | Commit `c5211311e34bf`: against disposable `postgres:18-alpine` (docker, `postgresql://…@localhost:55432/koduck_test`) the command exited 0 with `1 passed; 0 failed`; two advisory-serialized runners plus a second application succeeded with each 0009 object existing exactly once; the pre-migration seven-kind terminal Turn replayed exactly equal to the independently constructed expectation and identically after the second application (matching SHA-256 replay hashes); the valid correction round-tripped at sequence 8; self-reference, duplicate successor, cross-Turn scope, unknown target, and all three shape cases were rejected by their named constraints; the stalled-lock leg left schema state identical; the aborted transaction persisted nothing; a malformed external payload failed replay closed with `Unavailable`. |
| AC-4 | T-2 | The complete narrow slice retains existing quality, resource, timeout, governance, and CR-07 scope-exclusion contracts. | T-1/T-2 complete; approved base SHA and tested commit SHA recorded; no admission/projection/provider integration implementation; no unrelated dependency or source diff. | From repository root run `cargo fmt --all --check`; `cargo clippy -p koduck-ai --all-targets --all-features -- -D warnings`; `cargo test -p koduck-ai --all-targets --all-features`; `npm test --prefix tools/governance-validator`; and `npm run validate --prefix tools/governance-validator`; then run `git diff --name-status <approved-base-sha>...<tested-commit-sha>` and inspect `git diff <approved-base-sha>...<tested-commit-sha> -- <affected-paths>` against CR-07. | Every command exits 0; existing bounds/deadlines remain green; migration 0009 is the only migration addition; the routed diff contains no public route, correction event or admission operation, effective projection, provider correction semantics, dependency, deployment configuration, or unrelated behavior. | Command outputs, approved base SHA, tested commit SHA, routed name/status and content diffs, and governance report. | Pass | Approved base `88a8e4f` (ADR acceptance) to tested commit `c5211311e34bf`: all five commands exited 0 (fmt clean; clippy `-D warnings` clean; full suite 275 lib tests plus every integration binary passed with the database URL set; governance tests 145 pass / 0 fail; governance validation passed). The routed name/status diff contains exactly the declared affected paths plus two adjacent `payload_kinds` test compatibility arms and the shared test-migration registration; 0009 is the only migration addition; no public route, admission operation, correction event, projection, provider semantics, dependency, or deployment configuration appears in the diff. |

Allowed final check statuses are `Pass`, `Fail`, or `N/A — <specific reason>`.
`Fail` blocks completion. `N/A` is valid only when the check's stated trigger or
precondition demonstrably does not apply.

## Completion Checklist [Required]

| ID | Item | Completion Criterion | Expected Evidence | Status | Actual Evidence |
| --- | --- | --- | --- | --- | --- |
| A-1 | ADR approved | An eligible non-author approver, approval time, and exact `Approval Evidence: Approve` are recorded; any optional Approval Context Revision is informational, non-binding, and exactly represents the approved document | ADR metadata | Complete | Approved by `@linhai` (the named Decision Owner and Required Approver, distinct from author `@codex`) at `2026-08-25T22:43:30+08:00` after self-declaring `@linhai` in the task context and responding with exact `Approve` for the revised content including the completed Risk Coverage Matrix cell; the earlier `2026-08-25T22:36:27+08:00` approval and its invalidation are preserved in the Change Log. No Approval Context Revision is recorded because the approved content is not yet represented by an immutable commit. |
| A-2 | Complete task delivered | Every declared subtask has actual implementation evidence, every applicable acceptance check is `Pass` with actual result and evidence, and together they satisfy the complete task outcome | Implementation Plan and Acceptance Checks rows | Complete | Commit `c5211311e34bf`: T-1 and T-2 are `Complete` with red-first evidence; AC-1 through AC-4 are `Pass`; the typed representation, additive migration, fail-closed codec, and immutable ordered raw replay are delivered with no correction admission or consumer semantics (CR-07). |
| A-3 | Reciprocal ADD link synchronized, when applicable | The selected candidate records this exact ADR path, this ADR records the exact ADD path and candidate ID, both references agree, and the candidate reaches `Complete` only with this ADR's `Complete` or `Verified` status | Exact ADD path, candidate ID, ADR path, and Git blob or commit | Complete | CAND-3 is `Selected` in `docs/architecture/ADD-0001-ai-service-codex-alignment.md`, names this exact ADR path, and this ADR names that ADD and CAND-3; the candidate is not prematurely `Complete`. |
| A-4 | Requirement levels satisfied | Every required section is complete, every conditional trigger is assessed and completed or marked `N/A — <reason>`, and optional sections are complete or removed | Structured document review | Complete | Structured review on 2026-08-25 found no blank required content or unassessed conditional trigger. |
| A-5 | Acceptance checks are decidable | Every check names one subtask, preconditions or input, deterministic method, exact expected result, and evidence; no unqualified subjective criterion remains | Structured acceptance-check review | Complete | AC-1 through AC-4 name exact planned test targets or routed commands, binary expected outcomes, and evidence. |
| A-6 | Engineering exceptions governed, when applicable | Every exceeded or waived engineering rule has one complete exception row, an accountable owner, a lifecycle, and verification evidence before approval; otherwise the conditional subsection records `N/A — <reason>` | Engineering Exceptions subsection and affected-file evidence | N/A — no exception planned | No rule is waived; existing large files receive narrow declarations/delegation and new codec behavior is isolated. |
| A-7 | Contract and baseline risks covered, when applicable | Every normative contract clause maps to an explicit check or deterministic test, and every required Risk Coverage Matrix row is complete before approval and reaches Pass or specific N/A before review-ready or completion | Contract-To-Check Traceability, Risk Coverage Matrix, acceptance checks, and stable evidence | Complete | CR-01 through CR-08 map to AC-1 through AC-4; all five baseline risk dimensions define deterministic ownership and outcomes or a specific N/A reason. Runtime evidence remains `Not Started` until implementation. |
| A-8 | Governance validation passed | The independent validator reports no required-section, template-field, lifecycle-status, index, reciprocal-link, or Mermaid contract error for this record and repository | `npm run validate --prefix tools/governance-validator` output | Complete | Exit 0 on 2026-08-25 after ADD-0001 reapproval; governance validation passed with ADD-0001 `Current`, narrowed CAND-3 `Selected` by this Proposed ADR, CAND-11 through CAND-13 still `Ready`, and the central ADR and architecture indexes synchronized with the current records. |

## Archival [Conditionally Required — Decision Status is `Rejected`, or Decision Status is `Deprecated` or `Superseded` and Implementation Status is final]

Archive this record in the same change that retires it when either Decision
Status is `Rejected` with Implementation Status `Not Applicable`, or Decision
Status is `Deprecated` or `Superseded` with Implementation Status `Verified`,
`Complete`, or `Not Applicable`. Before that trigger, retain this section as
inactive future-lifecycle guidance; its checklist does not affect acceptance or
implementation completion. When triggered:

- [ ] Move this file to
      `archive/ADR-0003-correction-item-schema-and-raw-replay.md` under this
      service ADR root.
- [ ] Update every code marker that cites this file's pre-archive path to the
      new archive path, or remove the marker if the governed code was deleted.
- [ ] If Decision Status is `Superseded`, set the replacement record's
      `Supersedes` field and this record's `Superseded By` field to each other's
      final repository-relative path.
- [ ] If no record supersedes this one, retain `Superseded By: None`.
- [ ] Update this record's single row in `docs/adr/INDEX.md` with the archived
      path, scope, and final status.
- [ ] Confirm no ADR or OCR outside an `archive/` directory, and no code marker,
      still cites the pre-archive path.

## Change Log [Required]

| Date | Change | Author |
| --- | --- | --- |
| 2026-08-25 | Proposed the original CAND-3 service-internal Full ADR for append-only correction and synchronized its reciprocal ADD candidate and central ADR index. | @codex |
| 2026-08-25 | Before acceptance or implementation, narrowed the ADR to correction Item schema/codec and raw replay, renamed it to match that outcome, removed authenticated admission/concurrency, effective projection, and provider integration from its contract and affected paths, and left CAND-11 through CAND-13 for later serialized ADRs after ADD reapproval. | @codex |
| 2026-08-25 | Recorded passing deterministic governance tests and full repository governance validation for the narrowed Proposed ADR and Draft ADD split. | @codex |
| 2026-08-25 | Addressed non-blocking structured review findings by making all focused acceptance commands exact libtest invocations and distinguishing CR-05 decoder failures from durable schema-constraint rejection and replay handling of malformed legacy/external data. | @codex |
| 2026-08-25 | Corrected current A-8 evidence after ADD-0001 reapproval, removed the unchanged architecture index from implementation scope, and made CR-07 absence claims a binary AC-4 routed-diff review instead of attributing them to the focused codec test. | @codex |
| 2026-08-25 | Approved by `@linhai` after self-declaring `@linhai` in the task context and responding with exact `Approve`; recorded Approval Time `2026-08-25T22:36:27+08:00`. No Approval Context Revision was recorded because the approved content was not represented by an immutable commit. | @linhai |
| 2026-08-25 | Before the acceptance change was committed, acceptance-stage governance validation found the Risk Coverage Matrix `Cancellation and interruption` row's Owning boundary cell incomplete: a reasoned `N/A` is permitted only in the applicability column for an Accepted record. Completed that cell to the concrete runtime startup migration transaction boundary. Because this edits Risk Coverage Matrix content approved at `2026-08-25T22:36:27+08:00`, the approval is invalidated at `2026-08-25T22:42:03+08:00`: preserved the prior Approver `@linhai`, Approval Time `2026-08-25T22:36:27+08:00`, and `Approval Evidence: Approve` in this Change Log, and reset Decision Status to `Proposed` with active approval fields to `Pending — reapproval required`. No other content changed. | @zcode |
| 2026-08-25 | Reapproved the revised record after `@linhai` responded with exact `Approve` in the task context; recorded Approval Time `2026-08-25T22:43:30+08:00` and set Decision Status to `Accepted` with Implementation Status `Not Started`. No Approval Context Revision is recorded because the approved content is not yet represented by an immutable commit. | @linhai |
| 2026-08-25 | Implemented T-1 and T-2 red-first in commit `c5211311e34bf` and recorded implementation completion: every subtask `Complete`, AC-1 through AC-4 `Pass`, and every Risk Coverage Matrix row `Pass` or reasoned `N/A`. | @zcode |
| 2026-08-26 | Addressed the automatic-review P2 finding on pull request 7 in commit `c9db8bdf64c2d`: the correction decoder now rejects a durable payload carrying any extra member instead of silently canonicalizing it, and AC-1 gained the two red-first extra-member fixtures; the contract copy wording was aligned. Evidence-only update, not approval-invalidating. | @zcode |
