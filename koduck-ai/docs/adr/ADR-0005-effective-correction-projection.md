# ADR-0005: Effective Correction Projection

## Metadata [Required]

- **Decision Status**: Accepted
- **Implementation Status**: Blocked
- **Date**: 2026-09-24
- **Author**: @codex
- **Decision Owner**: @linhai
- **Required Approver**: @linhai
- **Record Scope**: Service internal — koduck-ai
- **Approver [Conditionally Required — Decision Status is or has been `Accepted`]**: @linhai
- **Approval Time [Conditionally Required — Decision Status is or has been `Accepted`]**: 2026-09-24T10:01:41Z
- **Approval Evidence [Conditionally Required — Decision Status is or has been `Accepted`]**: Approve
- **Rejector [Conditionally Required — Decision Status is `Rejected`]**: N/A — Decision Status is `Accepted`
- **Rejection Time [Conditionally Required — Decision Status is `Rejected`]**: N/A — Decision Status is `Accepted`
- **Rejection Evidence [Conditionally Required — Decision Status is `Rejected`]**: N/A — Decision Status is `Accepted`
- **Rejection Reason [Conditionally Required — Decision Status is `Rejected`]**: N/A — this ADR has not been rejected
- **Retired By [Conditionally Required — Decision Status is `Deprecated` or `Superseded`]**: N/A — this ADR has not been retired
- **Retirement Time [Conditionally Required — Decision Status is `Deprecated` or `Superseded`]**: N/A — this ADR has not been retired
- **Retirement Evidence [Conditionally Required — Decision Status is `Deprecated` or `Superseded`]**: N/A — this ADR has not been retired
- **Retirement Reason [Conditionally Required — Decision Status is `Deprecated` or `Superseded`]**: N/A — this ADR has not been retired
- **Blocked From [Conditionally Required — Implementation Status is `Blocked`]**: Not Started
- **Blocker And Evidence [Conditionally Required — Implementation Status is `Blocked`]**: The implementation slice is delivered and locally verified (T-1 `Complete`; AC-1 through AC-6 `Pass`), but AC-7's Sonar command cannot pass for this revision under the pinned coverage selection: `tools/sonarqube/rust-coverage.sh` runs only `cand_11_correction_admission` and `postgres_cand_11`, and a Disposable Verification Execution probe on 2026-09-24 (that exact command inside the canonical disposable PostgreSQL fixture, report deleted after measurement) recorded `correction_projection.rs` at 105 executable lines with 0 covered, so the changed executable lines cover at most 68 of 178 (≈38%), below the required 80%. Updating that pinned selection is a pipeline-configuration change that ADR-0017 deliberately keeps out of its own scope and that this ADR's EP-08/affected-paths do not authorize, and the repository-wide ADR serialization gate forbids drafting the governing record while this ADR is non-terminal. `KODUCK_SONAR_TOKEN` is also absent from the direct shell (available only through the interactive-zsh fallback used by `git push`), and pushing is outside this task's authorization. Resolving the selection requires a repository-owner decision.
- **Blocker Owner [Conditionally Required — Implementation Status is `Blocked`]**: @linhai
- **Blocker Exit Or Recheck Criterion [Conditionally Required — Implementation Status is `Blocked`]**: The pinned `tools/sonarqube/rust-coverage.sh` target selection is updated to also run `koduck-ai/tests/cand_12_projection.rs` (or the owner records an equivalent authorized disposition for the selection). Then this record returns to `In Progress`; the next `git push` scans afresh, and AC-7 is satisfied by the complete admission line or a passing `check --revision <target>` for the exact pushed revision plus green required CI.
- **Related [Optional]**: [Trello requirement](https://trello.com/c/4WI4sszw); `koduck-ai/docs/adr/ADR-0003-correction-item-schema-and-raw-replay.md`; `koduck-ai/docs/adr/ADR-0004-authenticated-correction-admission.md`; `docs/adr/ADR-0001-provider-neutral-turn-kernel.md`; `docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md`; `docs/adr/ADR-0005-provider-delta-coalescing-and-512-item-turn-budget.md`
- **Architecture Source [Conditionally Required — product demand]**: `docs/architecture/ADD-0001-ai-service-codex-alignment.md` — CAND-12
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

CAND-3 preserves original Items and append-only corrections in raw replay.
CAND-11 admits a correction to a terminal, subject-owned Turn and guarantees a
linear chain rooted at a UserMessage or AgentMessageDelta. A later consumer
needs the last correction's meaning at the original Item's position; emitting
each correction as a new conversational message would repeat superseded text
and move the corrected meaning after intervening events.

This ADR selects CAND-12 from the Current ADD. Its complete outcome is one pure
C-2 projection callable by later consumers. It owns chain interpretation,
source validation, output order, and typed rejection. CAND-13 owns the later
history-to-provider integration. No production caller is connected here.

At baseline `288e2cccf8e0b8c3c7cdd501c490f7373b89e653`, domain `Item` has
identity, Turn-local sequence, and payload, but no tenant, subject, Thread, or
Turn fields. `TurnHistory::prior_thread_items` returns a flattened `Vec<Item>`.
Its SQLx implementation is `SqlxPostgresExecutor::prior_thread_items_async`.
Neither sequence resets nor terminal Items can prove the original scope of
that flattened data. Projection therefore receives explicit source provenance
and operates on one Turn at a time. The future integrating reader must retain
that provenance before flattening; this ADR does not change the reader port.

## Scope [Required]

In scope:

- A service-internal, synchronous projection function with scoped borrowed
  inputs, a distinct borrowed effective-view type, and typed errors.
- Validation of source scope, raw replay structure, earlier predecessor
  relationships, supported roots, and deterministic chain-tip substitution.
- Original-position preservation for every non-correction Item, including
  terminal, usage, approval, and Tool Items.
- Minimal application module exports and a crate-private reference-iterator
  seam that reuses the existing raw-replay validator without cloning payloads.
- Focused semantic tests, preservation checks, and the evidence for this one
  implementation pull request to `dev`.

Out of scope:

- Store queries, schema/migrations, correction admission, new writes, raw replay
  changes, snapshots, forks, or Thread mutation admission.
- Provider serialization, agent-delta grouping, ModelInput/runner wiring,
  routes, REST/SSE contracts, UI, Memory, and automatic compaction.
- New dependencies, configuration, deployment, operational rollback, or new
  execution, approval, lease, retry, or cancellation authority.

## Tensions, Constraints, And Open Questions [Required]

### Identified Tensions [Conditionally Required — competing goals or trade-offs exist]

| ID | Tension | Impact | Decision |
| --- | --- | --- | --- |
| TN-1 | Consumers need corrected content while canonical evidence must retain every append. | Returning altered durable Items could obscure their provenance or be written back accidentally. | Return a distinct borrowed view with original and effective source identities. |
| TN-2 | Existing Item values omit ownership and Turn identity. | A flat vector cannot establish whether a target belongs to the same Turn. | Require per-entry source scope and one expected scope; never infer provenance from content or sequence. |
| TN-3 | A correction changes one stored agent delta, while providers group deltas into messages. | Concatenating here would duplicate provider policy and change correction granularity. | Replace only the corrected Item's content; preserve each delta's position and kind. |
| TN-4 | Corrupt history may have an apparently usable prefix. | Returning partial output would silently discard evidence. | Validate the complete supplied input before returning any successful projection. |

### Constraints [Required]

- CAND-3 CR-01 through CR-05 retain representation, raw replay, and structural
  rejection authority. Its CR-07 non-integration rule described that slice;
  this separate ADR adds only the CAND-12 consumer explicitly delegated there.
- CAND-11 CA-03 fixes the supported roots and strictly earlier ancestry;
  CA-05/CA-09 preserve original Items, terminals, and execution authority.
- Item sequence is local to a Turn and need not be adjacent. Corrections may
  follow its terminal Item. Projection must not stop at the terminal.
- The same authenticated read that obtains a row must supply its provenance.
  These internal provenance types are not credentials and perform no ownership
  lookup. A fabricated scope label cannot authenticate a row; the later
  integrating store boundary remains responsible for truthful provenance.
- Existing provider aggregate limits, the 512-Item/1-MiB execution budget,
  and CAND-11's admission/read caps remain owned by their existing boundaries.
  This slice does not connect a path that could bypass those limits.
- The common engineering and Rust standards apply. No engineering exception
  or additional dependency is proposed.

### Open Questions [Conditionally Required — material questions exist or were resolved during drafting]

| ID | Question | Owner | Due | Status | Resolution and Evidence |
| --- | --- | --- | --- | --- | --- |
| Q-1 | Does a correction replace one delta or an assembled assistant message? | @linhai | Before ADR acceptance | Resolved | One Item; CAND-11 CA-03 explicitly fixes this granularity. Provider grouping remains CAND-13. |
| Q-2 | Can the existing flattened history establish same-Turn scope? | @linhai | Before ADR acceptance | Resolved | No. Baseline `Item` and `prior_thread_items_async` omit row scope from the returned value. EP-01 requires explicit provenance; this proposal does not infer it or change persistence. |
| Q-3 | Does pure projection need another timer or cancellation owner? | @linhai | Before ADR acceptance | Resolved | No. CAND-12 explicitly has no independent writer or timeout owner. EP-07 bounds traversal by supplied input; future runtime consumers own admission and stop handling. |

## Decision Drivers [Required]

1. **Deterministic meaning**: Every original text Item has exactly one effective
   value from its own chain.
2. **Preserved evidence**: Projection never changes canonical identities,
   ordering, payloads, or terminal outcomes.
3. **Explicit scope**: Unrelated histories cannot supply a missing predecessor.
4. **Narrow integration**: One C-2 policy can be tested without a database,
   provider, server, or runtime owner.
5. **Proportional work**: Long chains use bounded iterative indexing and borrow
   payloads instead of repeatedly walking or copying them.

## Options Considered [Required]

### Option: Substitute content during durable replay

The C-6 reader would replace an original Item's content while returning replay.

Pros:

- Existing history consumers immediately receive corrected content.

Cons:

- Violates the raw replay contract and hides original evidence.
- Couples persistence to effective-history policy and expands this candidate.

### Option: Resolve chains inside provider serialization

The provider adapter would interpret correction links when building model input.

Pros:

- Places corrected content directly at its first planned consumer.

Cons:

- Mixes CAND-12 policy with CAND-13 provider integration.
- A later Memory or compaction consumer would need the same policy again.

### Option: Pure scoped projection with a distinct effective view

The C-2 application policy would validate one scoped Turn and return borrowed
effective Items for later consumers.

Pros:

- Keeps raw data intact and source provenance visible.
- Makes chain semantics and every rejection independently testable.
- Preserves the accepted separation from provider and storage changes.

Cons:

- Requires a small new internal input/output API.
- Runtime use awaits CAND-13 and its provenance-preserving history integration.

## Decision [Required]

**Selected option**: Pure scoped projection with a distinct effective view.

**Rationale**: A single deterministic transformation owns corrected meaning.
Borrowed source references preserve the relationship between that meaning and
the immutable evidence, while explicit input scope supports rejection before
the transform can combine unrelated data.

### Normative Contract Clauses [Required]

- **EP-01 — Scoped input**: `project_corrections` receives one expected
  `ProjectionScope` (tenant, subject, Thread, and Turn) and an ordered slice of
  `ScopedProjectionItem` entries. Each entry borrows one canonical `Item` and
  the scope reported by its authenticated source. Every scope component must
  equal the expected component; otherwise return
  `ProjectionError::ScopeMismatch { index }` with only the zero-based input
  index of the first mismatching entry, before structural validation. It must
  never carry either the expected or observed scope value. Empty input returns
  an empty projection. The supplied slice represents the complete source Turn
  replay at the caller's chosen read point; this pure function neither fetches
  missing rows nor certifies completeness, source authenticity, or freshness.
  Flat multi-Turn history must not be relabeled as one Turn to satisfy this
  input contract.
- **EP-02 — Raw structure**: Apply the existing `validate_raw_replay` semantics
  to the borrowed Items: positive strictly increasing sequences, unique Item
  identities, existing non-self targets, and at most one direct successor per
  predecessor. Preserve sequence gaps. Wrap its exact typed cause in
  `ProjectionError::InvalidReplay(RawReplayStructureError)` so callers can
  inspect the typed inner cause; retain the existing validator's rejection
  order. It must behave identically for existing slice callers after extracting
  the shared reference-iterator seam. Projection does not sort or repair input.
- **EP-03 — Valid ancestry**: After EP-02 succeeds, inspect correction entries
  in input order. Each predecessor must be strictly earlier in that Turn. A
  later target returns `ForwardReference`. Every chain must terminate at a
  `UserMessage` or `AgentMessageDelta`; a different root returns
  `UnsupportedRoot`. A correction may target an earlier correction in the
  same linear chain. Cycles cannot be accepted: self cycles fail EP-02, and
  any longer cycle contains a forward edge rejected here. A missing or foreign
  target is never recovered from another scope. When several faults exist,
  scope validation precedes raw structure, which precedes this ordered ancestry
  pass; each pass returns its first violation under the stated order.
- **EP-04 — Effective value and provenance**: Return exactly one
  `EffectiveItem` for each non-correction input Item, in their original input
  order. Each view retains a reference to that original Item, including its
  exact identity, sequence, and payload kind. For a corrected text root, its
  effective content is the exact untrimmed UTF-8 content of the last correction
  in that root's chain, and its effective source is that correction Item. For
  uncorrected text, source and content come from the original Item. No
  admission-time content limit is rechecked here: CA-01 owns the nonblank and
  65,536-byte correction-content rules, while CA-06 owns stored-payload read
  limits. Projection neither trims nor independently length-checks supplied
  text; it preserves the validated source bytes exactly. No intermediate or
  final Correction Item is a separate output element. The
  type exposes original/source provenance and effective content through
  read-only accessors; it is distinct from a durable `Item` and introduces no
  persistence or wire conversion.
- **EP-05 — Preserved non-text and delta semantics**: Usage, terminal,
  ApprovalStatus, ToolCall, and ToolResult Items retain their exact payloads
  and positions relative to every other non-correction Item. Their effective
  source is themselves and their text-content accessor returns `None`.
  Correcting an AgentMessageDelta preserves its kind and changes only that
  delta's effective text; adjacent deltas are not joined. Process corrections
  after a terminal Item. Never infer a new terminal, approval, dispatch, or
  Tool outcome from projected content.
- **EP-06 — Atomic result and diagnostics**: Return either the complete
  projection or one typed error, never a partial prefix, fallback projection,
  or silently dropped malformed chain. All success and failure paths leave
  the input unchanged. Repeated calls on equal scoped input yield equal
  observable values and provenance. Errors carry only the enumerated typed
  category/cause and the zero-based index specified by EP-01; their Display
  and Debug forms contain no original or replacement content, tenant/subject
  text, or raw payload. Projection emits
  no logs or external diagnostics of its own.
- **EP-07 — Work and resource ownership**: Use only call-local metadata and
  borrowed payloads. A fixed number of input passes plus indexed root/tip
  resolution must bound retained metadata and output by O(n), where n is the
  supplied Item count, without recursive traversal, repeated whole-chain
  scans, content cloning, or concatenation. The proposed indexing yields
  expected O(n) hash-based raw validation followed by O(n log n) ordered-map
  indexing, giving expected O(n log n) overall work; this is not a wall-clock
  guarantee. Output count equals n minus the number of correction entries.
  No independent history-size cap, truncation, timer, retry, lock,
  asynchronous task, cache, or cancellation state is introduced.
  Callers own bounded source acquisition and later context admission; this
  in-memory transform does not prove those external limits or prompt-stop
  behavior.
- **EP-08 — Integration boundary**: The implementation changes only the C-2
  projection, its necessary module exports, the behavior-preserving raw
  validator seam, tests, and governing documentation. The existing raw-replay
  API, durable data, admission, ModelInput, runner, provider serialization,
  routes, configuration, dependencies, and execution authority retain their
  behavior. No production call site invokes the new projection in this slice.

### Algorithm And Reuse Design [Required]

Place the policy in `application/correction_projection.rs`, with intentional
exports from `application/mod.rs`. `ProjectionScope` uses existing TenantId,
ThreadId, TurnId, and the subject from TrustContext; it does not accept a new
external identity format. `ScopedProjectionItem` and `EffectiveItem` borrow
their sources. The scope wrapper does not expand the domain Item schema.

Extract a crate-private `validate_raw_replay_refs` entry point in
`domain/item_correction.rs` accepting a cloneable iterator of `&Item`.
Clone it before consumption so each validation pass starts at the same first
Item; `items.iter()` and `entries.iter().map(|entry| entry.item())` are
examples of suitable restartable traversals. Keep public
`validate_raw_replay(&[Item])` as the same-contract caller of that shared
implementation. This limited seam lets projection preserve per-entry scope
without allocating a cloned `Vec<Item>` or copying structural policy.
Existing raw-validator semantic fixtures must still return their exact errors.

After validating all scopes and raw structure, make one forward pass with a
call-local ItemId index. For each textual root, create its output position;
for each correction, resolve the already-seen predecessor's root, update that
root's selected source, and index the correction to that root. An absent
already-seen predecessor after EP-02 means a forward reference. A predecessor
root of another kind is rejected. Non-correction Items retain their output
positions; all intermediate state is private until every entry validates.
Ordered-map lookups avoid per-root history scans. No final output is assembled
by map iteration, so hash/map iteration order cannot reorder messages.

For example, the input below produces four effective entries:

| Raw Item / sequence | Payload | Effective result |
| --- | --- | --- |
| U / 1 | UserMessage `old` | Original U / 1, effective source C2, content `latest` |
| A / 2 | AgentMessageDelta `A` | Original A / 2, effective source C3, content `X` |
| B / 4 | AgentMessageDelta `B` | Original B / 4, effective source B, content `B` |
| T / 8 | Terminal | Original T / 8 and exact terminal payload |
| C1 / 9 | Correction of U: `new` | No separate output |
| C2 / 10 | Correction of C1: `latest` | No separate output |
| C3 / 11 | Correction of A: `X` | No separate output |

### Consequences [Required]

Positive: corrected meaning has one owner, raw evidence remains usable, and
later consumers can inspect both the original and selected source.

Negative: a caller must retain source scope and source lifetimes. A corrupt
chain rejects the entire supplied projection, and this slice alone does not
change a user's model response.

Mitigations: small borrowed types, explicit typed rejection, unchanged raw
replay for diagnosis, and a separate CAND-13 integration decision. No fallback
to uncorrected or truncated content is permitted on projection failure.

## Implementation Plan [Required]

**Complete task outcome**: One independently reviewable implementation PR
delivers the EP-01 through EP-08 pure scoped projection and its deterministic
tests, with unchanged raw replay and no runtime integration.

**Primary implementation boundary**: C-2 application correction-projection
policy inside `koduck-ai`. The domain validator seam is the sole adjacent
supporting change and preserves its public behavior.

Allowed subtask statuses: `Not Started`, `In Progress`, `Blocked`, `Complete`,
or `N/A — <specific reason>`.

| ID | Objective or deliverable | Included scope | Status | Actual implementation evidence |
| --- | --- | --- | --- | --- |
| T-1 | Deliver the scoped correction projection, typed failures, and semantic contract tests. | Projection module and exports; reference-iterator raw-validator seam; focused fixtures and source-preservation checks. | Complete | Delivered 2026-09-24 on task branch `codex/cand-12-effective-correction-projection` (base `288e2cccf8e0b8c3c7cdd501c490f7373b89e653`). Source blobs: `correction_projection.rs` `f2740d76f845674a98185009835c8ee1ab5686f8` (294 lines), `item_correction.rs` `aa121978ed5ce8acc6a712f01d0512fd64bd6c2d` (160 lines, seam only), `application/mod.rs` `f08f6ea9439a5a552e23bff93b51095e9f115abe` (92 lines, exports + marker), test root `ae923447bc3afd5c9e8df81abadf679df20552d0` (950 lines), fixtures `89085de4575da6755931817f945f34539db58484` (223 lines). Red phase: `cargo test -p koduck-ai --test cand_12_projection chain_tip_substitution -- --exact` failed with E0432 unresolved imports before implementation; green phase: every AC-1 through AC-6 command reports `1 passed; 0 failed; 0 ignored; 5 filtered out`. |

**Affected paths**: `koduck-ai/src/application/correction_projection.rs`
(new), `koduck-ai/src/application/mod.rs`,
`koduck-ai/src/domain/item_correction.rs`,
`koduck-ai/tests/cand_12_projection.rs` (new; focused child fixture modules
under `koduck-ai/tests/cand_12_projection/` only if needed for cohesion),
this ADR, `docs/adr/INDEX.md`, and
`docs/architecture/ADD-0001-ai-service-codex-alignment.md`.

The ADR itself is the authoritative EP contract; no duplicated contract copy
is required. Every new or modified source file cites this ADR. The focused
tests start with a failing one-root/one-correction substitution case before
production implementation, then cover the remaining cases below.

### Stable Implementation Touchpoints [Conditionally Required — source or configuration implementation]

| Path | Stable symbol or contract anchor | Key code excerpt, when needed | Purpose | Source revision |
| --- | --- | --- | --- | --- |
| `koduck-ai/src/domain/item_correction.rs` | `koduck_ai::domain::item_correction::{ItemCorrection, RawReplayStructureError, validate_raw_replay}` | N/A — stable symbols suffice | Existing representation and reusable structural policy; add only the crate-private iterator seam. | `288e2cccf8e0b8c3c7cdd501c490f7373b89e653` |
| `koduck-ai/src/domain/mod.rs` | `koduck_ai::domain::{Item, ItemPayload, TrustContext}` | N/A — stable symbols suffice | Read-only dependency: Item omits scope, and corrections replace one supported text Item. No edit planned. | `288e2cccf8e0b8c3c7cdd501c490f7373b89e653` |
| `koduck-ai/src/application/correction_projection.rs` | Proposed `koduck_ai::application::{project_corrections, ProjectionScope, ScopedProjectionItem, EffectiveItem, ProjectionError}` | N/A — proposed symbols and EP clauses define the new boundary | Sole owner of scoped projection and rejection; names are the proposed internal API. | Delivered blob `f2740d76f845674a98185009835c8ee1ab5686f8` (2026-09-24) |
| `koduck-ai/src/application/mod.rs` | `koduck_ai::application` module exports | N/A — stable module anchor suffices | Expose only the projection's consumer-facing service-internal types/function. | `288e2cccf8e0b8c3c7cdd501c490f7373b89e653` |
| `koduck-ai/src/application/ports.rs` | `koduck_ai::application::{TurnHistory::prior_thread_items, ModelInput}` | N/A — stable symbols suffice | Read-only boundary: flattened history is not a provenance source; integrating it belongs to CAND-13. | `288e2cccf8e0b8c3c7cdd501c490f7373b89e653` |
| `koduck-ai/src/adapters/history/postgres/sqlx_executor.rs` | `SqlxPostgresExecutor::prior_thread_items_async` | N/A — stable symbol suffices | Read-only implementation evidence: the current ordered query joins Turns but returns no Turn identity with each decoded Item. CAND-13 owns a provenance-preserving read. | `288e2cccf8e0b8c3c7cdd501c490f7373b89e653` |
| `koduck-ai/tests/cand_12_projection.rs` | Proposed tests named in AC-1 through AC-6 | N/A — named semantic tests suffice | Exercise the real pure function and existing raw validator, with no database/provider doubles. | Delivered blobs `ae923447bc3afd5c9e8df81abadf679df20552d0` (root) and `89085de4575da6755931817f945f34539db58484` (fixtures, 2026-09-24) |

**Migration and rollback strategy [Conditionally Required — this replaces or
changes existing behavior]**: No data migration is required. The supporting
validator extraction must preserve all existing results. If verification
fails, do not promote the implementation; withdraw the new module/exports and
restore the original validator implementation. Retain every durable correction
and the existing schema. No deployment or operational rollback is authorized.

### Engineering Exceptions [Conditionally Required — an engineering rule is exceeded or waived]

N/A — no engineering rule is waived. Keep the new module and tests cohesive
within the common limits; measure affected units during implementation.

### Key State And Invariant Matrix [Required]

The entry point for all rows is `application::project_corrections`; C-2 owns
effective meaning. C-6 retains source-authenticity and durable-state ownership.
The existing raw validator's entry point retains its CAND-3 contract.

| ID | Precondition / state | Action or transition | Expected observable outcome | Invariant and owner | Check / verification gap |
| --- | --- | --- | --- | --- | --- |
| KS-1 | Empty or valid uncorrected scoped replay | Project once, then repeat | Empty result or one unchanged view per Item, equal on repeat | C-2 preserves original order/identity and never mutates input | AC-1 |
| KS-2 | One or several interleaved linear correction chains, including post-terminal corrections | Extend a chain in a new input snapshot and project each snapshot | Only the affected root's selected content/source changes; all root positions persist | C-2 has one effective value per root and retains exact bytes | AC-2 |
| KS-3 | Adjacent agent deltas and non-text/Tool Items | Correct one text root | Only that delta's content changes; non-text payloads and positions persist | C-2 never groups messages or changes C-5 authority/terminal outcomes | AC-3 |
| KS-4 | Invalid order/identity/target/ancestry, including a valid prefix before corruption | Attempt projection | Exact typed error and no output prefix | C-2 fails closed; C-6 raw input remains unchanged | AC-4 |
| KS-5 | One provenance component differs, or a target exists only in another scope | Attempt projection, then retry with a valid independently supplied input | ScopeMismatch or UnknownCorrectionTarget; valid later input succeeds | C-2 never borrows another scope or retains failed-call state | AC-5 |
| KS-6 | A long chain or many independent roots; repeated or parallel callers share immutable input | Project and discard results | Expected values/provenance, n minus corrections outputs; no retained shared state | C-2 keeps O(n) metadata and borrows content; C-6 input is immutable | AC-6 |

Timeout/cancellation transitions are N/A for this synchronous transformation
with no I/O, task, effect, or independent control owner. Production source
freshness, context budgets, and runtime stop behavior are explicit integration
gaps owned by later candidates; this ADR claims no verification of them.

## Contract-To-Check Traceability [Conditionally Required — source or configuration implementation]

| Clause ID | Authoritative contract path and heading | Exact normative requirement | Acceptance check or deterministic test IDs | Explicit coverage method |
| --- | --- | --- | --- | --- |
| EP-01 | This ADR — Normative Contract Clauses | One expected scope; every source component matches; scope error carries only first mismatching zero-based index; no inferred or fetched provenance. | AC-1, AC-5, AC-7 | Empty input; individual scope drift and mixed entries with exact index; foreign-only target fixtures; input/API boundary inspection. The prohibition on relabeling flat multi-Turn history is a future CAND-13 caller obligation: AC-7 checks there is no production caller in this slice, and no direct runtime test is claimed here. |
| EP-02 | This ADR — Normative Contract Clauses | Preserve exact raw validation, sequence gaps, and inspectable typed causes. | AC-1, AC-4, AC-7 | Compare production slice validator and pattern-matched InvalidReplay causes for each structural error; existing CAND-3 tests remain green. |
| EP-03 | This ADR — Normative Contract Clauses | Strictly earlier linear ancestry reaches a supported root; defined rejection precedence. | AC-2, AC-4, AC-5 | Repeated/interleaved chains, unsupported roots, forward edges, self/long cycles, combined-fault fixtures. |
| EP-04 | This ADR — Normative Contract Clauses | One ordered view per non-correction Item with exact original and selected-source provenance/content; admission-time content limits are not rechecked. | AC-1, AC-2, AC-6 | Exact identity/sequence/kind/content/source vectors, including surrounding whitespace and multibyte bytes; source-reference checks, no duplicated correction output or added content validation. |
| EP-05 | This ADR — Normative Contract Clauses | Preserve non-text/Tool Items, delta granularity, and post-terminal corrections. | AC-2, AC-3 | Each Item variant and split-delta fixture, exact payload comparison, original relative order. |
| EP-06 | This ADR — Normative Contract Clauses | Complete result or error; immutable input; deterministic replay; redacted errors. | AC-1, AC-4, AC-5, AC-6 | Before/after full input equality, failed-call recovery, repeated/parallel outcomes, Display/Debug sentinel checks. |
| EP-07 | This ADR — Normative Contract Clauses | Call-local O(n) metadata, borrowed content, iterative indexed traversal, no new execution control. | AC-6, AC-7 | Large semantic fixtures and pointer provenance; deterministic source audit of allocation, loop, and ownership boundaries. |
| EP-08 | This ADR — Normative Contract Clauses | Limit changes to the declared projection boundary and preserve existing runtime behavior. | AC-3, AC-7 | All-variant preservation plus routed tests and scoped diff/call-site inspection. |
| CR-01 | `koduck-ai/docs/adr/ADR-0003-correction-item-schema-and-raw-replay.md` — Normative Contract Clauses | Reuse typed corrections and structural checks; no canonical rewrite, repair, or replay-policy change. | AC-1, AC-4, AC-7 | Existing raw validator outcomes, complete input equality, unchanged durable adapters/schema, CAND-3 regression suite. |
| CR-02 | `koduck-ai/docs/adr/ADR-0003-correction-item-schema-and-raw-replay.md` — Normative Contract Clauses | Reuse typed corrections and structural checks; no canonical rewrite, repair, or replay-policy change. | AC-1, AC-4, AC-7 | Existing raw validator outcomes, complete input equality, unchanged durable adapters/schema, CAND-3 regression suite. |
| CR-03 | `koduck-ai/docs/adr/ADR-0003-correction-item-schema-and-raw-replay.md` — Normative Contract Clauses | Reuse typed corrections and structural checks; no canonical rewrite, repair, or replay-policy change. | AC-1, AC-4, AC-7 | Existing raw validator outcomes, complete input equality, unchanged durable adapters/schema, CAND-3 regression suite. |
| CR-04 | `koduck-ai/docs/adr/ADR-0003-correction-item-schema-and-raw-replay.md` — Normative Contract Clauses | Reuse typed corrections and structural checks; no canonical rewrite, repair, or replay-policy change. | AC-1, AC-4, AC-7 | Existing raw validator outcomes, complete input equality, unchanged durable adapters/schema, CAND-3 regression suite. |
| CR-05 | `koduck-ai/docs/adr/ADR-0003-correction-item-schema-and-raw-replay.md` — Normative Contract Clauses | Reuse typed corrections and structural checks; no canonical rewrite, repair, or replay-policy change. | AC-1, AC-4, AC-7 | Existing raw validator outcomes, complete input equality, unchanged durable adapters/schema, CAND-3 regression suite. |
| CA-03 | `koduck-ai/docs/adr/ADR-0004-authenticated-correction-admission.md` — Normative Contract Clauses | Supported text roots, earlier ancestry, one-Item replacement, immutable original state, no execution authority. | AC-2, AC-3, AC-4, AC-7 | Root/kind fixtures and preservation checks; unchanged write paths plus existing regression suite. |
| CA-05 | `koduck-ai/docs/adr/ADR-0004-authenticated-correction-admission.md` — Normative Contract Clauses | Supported text roots, earlier ancestry, one-Item replacement, immutable original state, no execution authority. | AC-2, AC-3, AC-4, AC-7 | Root/kind fixtures and preservation checks; unchanged write paths plus existing regression suite. |
| CA-09 | `koduck-ai/docs/adr/ADR-0004-authenticated-correction-admission.md` — Normative Contract Clauses | Supported text roots, earlier ancestry, one-Item replacement, immutable original state, no execution authority. | AC-2, AC-3, AC-4, AC-7 | Root/kind fixtures and preservation checks; unchanged write paths plus existing regression suite. |

The inherited rows cover clauses whose behavior this projection consumes or
preserves. The remaining clauses stay with their accepted owners:

| Clause | Why this slice does not implement or remap it |
| --- | --- |
| CR-06 | Migration idempotency and earlier-row upgrade are CAND-3 storage behavior; this slice changes no migration or row. |
| CR-07 | Its exclusion of consumer integration defined the CAND-3 implementation slice. This separately selected CAND-12 ADR implements only projection and still leaves provider integration to CAND-13. |
| CR-08 | Retaining correction data and a verified reader/schema pair during withdrawal remains CAND-3 recovery; this slice has no schema change or data deletion. |
| CA-01, CA-02 | Command validation, caller ownership, and terminal admission remain in the unchanged CAND-11 write path. Projection trusts scoped source provenance but grants no admission authority. |
| CA-04 | Stable identity retries remain in the unchanged correction transaction; projection performs no lookup or write retry. |
| CA-06 | The 4,096-ancestor admission and 1-MiB stored-payload read caps belong to the CAND-11 writer. EP-07 deliberately adds no independent projection cap. |
| CA-07, CA-08 | Database deadlines, reconciliation, and commit-time cancellation remain in the unchanged CAND-11 write path; this synchronous projection has no operation or effect to reconcile. |

Existing provider contracts and leases also remain unchanged; their
implementation proof is not reassigned to this pure function.

## Risk Coverage Matrix [Conditionally Required — source or configuration implementation]

| Risk dimension | Applicability and scenario, or specific N/A reason | Owning boundary | Deterministic verification method | Exact expected result | Acceptance check IDs | Status | Actual evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Concurrency and ordering | Applicable — interleaved correction chains, repeated/parallel calls, and unordered input. | C-2 pure projection | Chain/order fixtures and four barrier-started calls sharing immutable input. | Exact root-order/source vectors; independent results; invalid order rejected; input unchanged. | AC-1, AC-2, AC-4, AC-6 | Pass | 2026-09-24: AC-1/AC-2/AC-4/AC-6 each `1 passed; 0 failed; 0 ignored; 5 filtered out`; interleaved roots, repeated calls, and four `std::thread::scope` barrier workers returned baseline-equal vectors over unchanged input. |
| Timeout and deadline | N/A — no I/O, wait, async execution, timer, or deadline owner exists in this synchronous pure boundary. | C-2 projection; future integration owns its runtime budget | AC-7 confirms no new wait or timer; AC-6 covers traversal growth. | No independently pending operation or invented deadline outcome. | AC-6, AC-7 | N/A — synchronous transformation without a deadline owner | Design applicability assessed; the EP-07 boundary audit on blob `f2740d76f845674a98185009835c8ee1ab5686f8` confirmed no wait, timer, or deadline owner in the delivered slice. |
| Cancellation and interruption | N/A — no task, side effect, lock, or cancellation lifecycle is created; CAND-12 explicitly has no independent control owner. | C-2 projection; caller owns continuation/stop decisions | AC-7 audits no task/control ownership; input-preservation tests apply. | Dropping results changes no input or external state; no background work survives a call. | AC-6, AC-7 | N/A — no independent cancellation lifecycle | Design applicability assessed; the boundary audit confirmed no task, lock, or cancellation lifecycle; input-preservation assertions passed in AC-1/AC-4/AC-5/AC-6. |
| Resource bounds and backpressure | Applicable — deep chains, many roots, large borrowed content, or repeated calls. | C-2 in-memory policy | Long-chain/many-root fixtures, borrowed-source assertions, allocation/loop ownership audit. | n minus corrections outputs, O(n) metadata, borrowed payloads, iterative traversal and no new queue/cap/truncation. | AC-6, AC-7 | Pass | 2026-09-24: AC-6 `1 passed; 0 failed; 0 ignored; 5 filtered out`; 4,097-node chain, 8,192 entries over 1.31 MiB, n-minus-corrections counts, pointer-provenance into source payloads; source audit confirmed one O(n) `Vec` plus one `HashMap`, one iterative pass, no cloning, recursion, per-root scans, cache, or cap. |
| Framework or trust-boundary rejection | Applicable — mislabeled scope, unsupported roots, or corrupt relationships reach the pure boundary; no framework adapter is changed. | C-2 input-validation boundary; C-6 owns authentic provenance | Call the production function with each invalid fixture and safe diagnostic sentinels. | ScopeMismatch/InvalidReplay/ForwardReference/UnsupportedRoot as specified; no partial output or content disclosure. | AC-4, AC-5 | Pass | 2026-09-24: AC-4/AC-5 each `1 passed; 0 failed; 0 ignored; 5 filtered out`; every invalid fixture produced its exact typed error with no partial output, and Display/Debug omitted all content and scope sentinels. |

## Acceptance Checks [Required]

AC-1 through AC-6 name proposed integration tests in
`koduck-ai/tests/cand_12_projection.rs`; they are not present or claimed to pass
at draft time. Each focused command must run exactly its one named top-level
test and exit 0; `0 passed` fails the acceptance check even if Cargo exits 0.
Tests call the real projection and raw validator and assert semantic
values, not source/document wording. All scoped fixtures have explicit stable
IDs and positive Turn-local sequences unless that dimension is under test.

| Check ID | Subtask | Binary acceptance point | Preconditions or input | Verification method | Exact expected result | Expected evidence | Status | Actual result and evidence |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| AC-1 | T-1 | Empty and unchanged history retain identity/order/content deterministically. | Empty input; uncorrected supported text plus non-text Items with sequence gaps; same input projected twice. | `cargo test -p koduck-ai --test cand_12_projection unchanged_history -- --exact` | Exactly one named top-level test runs and passes; empty output or one view per Item in the exact input order; original/source IDs equal; exact content and payloads; repeated observations equal; before/after input equality. | Cargo summary with `1 passed; 0 failed; 0 ignored` and recorded filtered-out count; exact expected vectors and unchanged-input assertions. | Pass | 2026-09-24: `test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 5 filtered out`. `unchanged_history` asserted the empty deterministic result, per-view identity/sequence/payload equality with self provenance, exact untrimmed and multibyte contents, `None` accessors for usage and terminal, sequence gaps 1/3/7/11 preserved, repeated-call equality, and unchanged input. |
| AC-2 | T-1 | Linear corrections select exactly one last value at each original position. | One correction, repeated correction of a correction, interleaved independent roots, and the U/A/B/T/C1/C2/C3 example; new snapshots append a successor; whitespace and multibyte content preserved. | `cargo test -p koduck-ai --test cand_12_projection chain_tip_substitution -- --exact` | Exactly one named top-level test runs and passes; output contains every non-correction Item once; original identity/sequence/kind/order unchanged; source ID/content equal each chain tip; new snapshot changes only the affected root's source/content; post-terminal corrections applied; input unchanged. | Cargo summary with `1 passed; 0 failed; 0 ignored` and recorded filtered-out count; per-case identity/sequence/kind/source/content vectors and input snapshots. | Pass | 2026-09-24: `test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 5 filtered out`. `chain_tip_substitution` covered the single-link case, correction-of-correction, interleaved roots in original order, the ADR example producing exactly four entries with post-terminal corrections, an appended snapshot changing only the affected root, and pointer-provenance for exact untrimmed bytes. |
| AC-3 | T-1 | Non-text payloads and delta granularity survive projection unchanged. | Every existing non-correction ItemPayload variant, adjacent AgentMessageDelta Items, Tool call/result and approval projections, terminal before later corrections. | `cargo test -p koduck-ai --test cand_12_projection item_kind_preservation -- --exact` | Exactly one named top-level test runs and passes; only corrected text accessor values differ; each delta remains separate; non-text accessor is None, original/source are identical, full payloads and relative positions match; no new terminal or authority value. | Cargo summary with `1 passed; 0 failed; 0 ignored` and recorded filtered-out count; exhaustive fixture outcomes and exact payload/order comparison. | Pass | 2026-09-24: `test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 5 filtered out`. `item_kind_preservation` projected all fifteen non-correction payload fixtures with self provenance, kept adjacent deltas separate under correction of one delta, preserved Tool call/result and approval payloads and relative positions in mixed history, and applied a post-terminal correction without reinterpreting the terminal outcome. |
| AC-4 | T-1 | Invalid history produces the declared typed error with no output or mutation. | Zero/equal/decreasing sequences, duplicate IDs, absent target, self edge, branched predecessor, strictly ordered forward edge, two-node and longer cycles, unsupported root of every non-text kind, valid prefix followed by corruption, combined raw/ancestry faults. | `cargo test -p koduck-ai --test cand_12_projection corruption_rejection -- --exact` | Exactly one named top-level test runs and passes; EP-02 cases pattern-match InvalidReplay containing the exact NonIncreasingSequence/DuplicateItemIdentity/UnknownCorrectionTarget/SelfCorrection/DuplicateSuccessor cause; forward/long-cycle cases return ForwardReference; unsupported roots return UnsupportedRoot; raw errors precede ancestry errors; no Ok prefix and exact input equality. | Cargo summary with `1 passed; 0 failed; 0 ignored` and recorded filtered-out count; case-to-error matrix, direct raw-validator comparison, and before/after snapshots. | Pass | 2026-09-24: `test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 5 filtered out`. `corruption_rejection` asserted every InvalidReplay cause through pattern equality plus the same cause from `validate_raw_replay` directly, ForwardReference for the strictly ordered forward edge and two-node/longer correction cycles (raw validator `Ok` on each), UnsupportedRoot for usage/approval/tool-call/tool-result/terminal targets, a valid prefix followed by a forward edge returning no output with unchanged input, and NonIncreasingSequence taking precedence over ancestry faults. |
| AC-5 | T-1 | Scope rejection, failed-call recovery, and diagnostics satisfy EP-01/EP-06. | Change tenant, subject, Thread, and Turn separately; mix one foreign entry at a known nonzero position among local entries; duplicate Item IDs in separate scoped calls; target available only in a separately scoped input; scope mismatch combined with structural corruption; unique sensitive sentinels in text/identity fields. | `cargo test -p koduck-ai --test cand_12_projection scope_and_diagnostics -- --exact` | Exactly one named top-level test runs and passes; every mismatch returns ScopeMismatch with the exact first mismatching zero-based index (0 for individually changed first entries; the chosen nonzero position for a mixed slice) before any raw error; absent local target returns InvalidReplay(UnknownCorrectionTarget), never borrowed foreign content; separate scopes remain independent; later valid call succeeds; the error carries no scope values and its Display/Debug forms omit sentinels; every input unchanged. | Cargo summary with `1 passed; 0 failed; 0 ignored` and recorded filtered-out count; named scope/error/index cases, diagnostic capture, valid-after-failure observations. | Pass | 2026-09-24: `test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 5 filtered out`. `scope_and_diagnostics` drove tenant/subject/Thread/Turn drift to `ScopeMismatch { index: 0 }`, a foreign entry at position 2 ahead of a structural fault to `ScopeMismatch { index: 2 }`, independent same-identity histories across separate scopes, a foreign-only target rejected as InvalidReplay(UnknownCorrectionTarget) followed by a valid local retry, and four error variants whose Display and Debug omit all four sentinels. |
| AC-6 | T-1 | Long and repeated projections borrow sources and retain deterministic bounded structure. | One 4,097-node root/correction chain (the resulting length permitted after CAND-11 admits against 4,096 ancestors); 8,192 interleaved entries across independent chains with more than 1 MiB of total source text; repeated calls and four barrier-started scoped threads sharing immutable input. | `cargo test -p koduck-ai --test cand_12_projection bounded_borrowed_projection -- --exact`; inspect the implementation's loops, allocations, and borrow lifetimes against EP-07. | Exactly one named top-level test runs and passes; exact chain-tip outputs; count n minus corrections; original/effective text references point into selected input payloads; all four result vectors equal; dropping/repeating results leaves input unchanged; audit proves call-local O(n) metadata and iterative indexed passes, no cloned payloads, recursion, per-root whole-history traversal, cache, or new cap. | Cargo summary with `1 passed; 0 failed; 0 ignored` and recorded filtered-out count; source-reference assertions, result vectors, and revision-bound allocation/traversal audit; no machine-specific latency threshold. | Pass | 2026-09-24: `test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 5 filtered out`. `bounded_borrowed_projection` projected the 4,097-node chain to its exact tip with pointer-provenance into the source payload, 4,096 independent chains over 8,192 entries and 1.31 MiB of source text with per-view original/source identity vectors, and four barrier-started `std::thread::scope` workers plus repeated calls returning vectors equal to the baseline with unchanged shared input. The EP-07 source audit on blob `f2740d76f845674a98185009835c8ee1ab5686f8` confirmed one output-slot `Vec` plus one `HashMap<ItemId, ChainRoot>` (O(n) call-local metadata), a single iterative forward pass, finalization reading borrowed payloads without cloning, recursion, per-root scans, caches, timers, or caps. |
| AC-7 | T-1 | The declared slice and unchanged surrounding contracts pass routed verification. | Implemented revision and explicit base; isolated PostgreSQL URL available to existing database tests; disposable compiler output; recorded CI and local Sonar prerequisites. | Run the commands below; inspect the exact changed-path diff and projection exports/call sites, validator behavior, and engineering measurements. | Every command exits 0; database tests actually run; only declared source/supporting paths change; no production projection caller or source/provenance fabrication; existing raw-validator results and runtime contracts preserved; changed units meet engineering rules; exact-revision delivery gates satisfied before review-ready/completion. | Focused/full test reports, source commit, scoped diff/API/size audit, required CI check results and applicable automatic-review disposition, local Sonar evidence. | Fail | 2026-09-24 partial execution on task branch base `288e2cccf8e0b8c3c7cdd501c490f7373b89e653`: `cargo fmt --all --check` and `cargo clippy -p koduck-ai --all-targets --all-features -- -D warnings` exit 0; `cargo test -p koduck-ai --all-targets --all-features` exits 0 with 301 unit tests plus every integration target green, including `postgres_cand_11` (5 tests) and the other PostgreSQL targets that actually ran against the canonical disposable fixture database; `npm test --prefix tools/governance-validator` and `npm run validate --prefix tools/governance-validator` pass on the delivered document state; the changed-path diff is exactly the declared source/supporting paths; no production call site invokes `project_corrections`; `cand_3_correction_schema` stays green and the full suite preserves runtime contracts; measured units comply (largest function 28 lines, files 92/160/294/223/950 lines, nesting ≤3, complexity `N/A — no configured complexity tool`). The Sonar command is blocked by the recorded coverage-selection blocker: under the pinned `rust-coverage.sh` selection the probe measured 105 executable lines / 0 covered for `correction_projection.rs` (≈38% changed-line coverage overall), and `KODUCK_SONAR_TOKEN` is absent from the direct shell; CI confirmation and the automatic-review disposition remain future push-time requirements. `Fail` blocks completion and keeps this record `Blocked` until the owner resolves the selection. |

AC-7 commands, from the repository root:

```sh
cargo fmt --all --check
cargo clippy -p koduck-ai --all-targets --all-features -- -D warnings
cargo test -p koduck-ai --all-targets --all-features
npm test --prefix tools/governance-validator
npm run validate --prefix tools/governance-validator
python3 tools/sonarqube/gate.py check --revision HEAD
```

Matching successful pre-push evidence may satisfy the local Sonar command
under the existing gate contract. Use the canonical isolated database workflow;
never log its credentials. Disposable compiler output is removed at task end.
The current CI mapping is `koduck-ai-format` for formatting/governance,
`koduck-ai-clippy` for Clippy, and `koduck-ai-test-postgres` for the complete
Rust suite including database tests. Local Sonar is deliberately outside CI.
At delivery, confirm these checks are required and green for the exact pushed
revision; record configured automatic review or the canonical no-mechanism
N/A under AGENTS.md. Local document review below is not remote review coverage.

Allowed final check statuses are `Pass`, `Fail`, or `N/A — <specific reason>`.
`Fail` blocks completion. No implementation check is satisfied by the draft's
governance validation alone.

## Completion Checklist [Required]

| ID | Item | Completion Criterion | Expected Evidence | Status | Actual Evidence |
| --- | --- | --- | --- | --- | --- |
| A-1 | ADR approved | Eligible non-author approval is recorded with concrete identity, time, and exact Approve evidence. | ADR metadata and approval context. | Complete | In this ADR-0005 task, the named approver self-declared and approved with `@linhai Approve`; recorded @linhai, `2026-09-24T10:01:41Z`, and exact Approval Evidence `Approve` in metadata. |
| A-2 | Complete task delivered | T-1 is Complete, AC-1 through AC-7 Pass, and all applicable risk rows Pass. | Source revision and verification reports. | In Progress | T-1 is Complete and AC-1 through AC-6 are Pass with all three applicable risk rows Pass (2026-09-24 evidence above). AC-7 is `Fail` only because the pinned Sonar coverage selection cannot cover the new module and the resolution requires an owner decision outside this ADR's EP-08 scope; the record is `Blocked` with the recorded exit criterion. |
| A-3 | Reciprocal ADD link synchronized | CAND-12 and this ADR name each other; Selected while non-terminal, Complete only with this ADR Complete/Verified. | Exact paths, candidate ID, index state and validator result. | Complete | CAND-12 remains Selected; this ADR and the central index now agree on Accepted/Not Started and retain exact reciprocal paths. Recheck on every later lifecycle transition. |
| A-4 | Requirement levels satisfied | All required content is complete for the current stage; every conditional trigger assessed. | Structured document review. | Complete | Local structured review rounds 1 and 2 on 2026-09-24 confirmed required content and conditional assessments for acceptance. Round 2 also verified all three required option descriptions and the inherited-clause disposition table. See Supporting Notes for reviewed blobs. |
| A-5 | Acceptance checks are decidable | Every check has one subtask, explicit fixtures, method, exact results, and evidence. | Structured acceptance-check review. | Complete | Round 2 verified AC-4's typed inner-cause assertion and AC-5's exact first mismatch index and redacted diagnostics. The subsequent task review identified Cargo's zero-matched-test success case; AC-1 through AC-6 now each require evidence that exactly one named top-level test ran and passed. All seven checks name T-1; no implementation test is claimed to exist or pass. |
| A-6 | Engineering exceptions governed | No waived rule; actual affected units comply or an accepted exception exists before retaining it. | Engineering Exceptions and revision-bound unit measurements. | Complete | 2026-09-24 measurements on delivered blobs: largest executable unit `ChainProjection::absorb` 28 physical lines (limit 80); `project_corrections` 14, `reject_foreign_scope` 16, `finish` 11, `effective_content` 9; files `application/correction_projection.rs` 294, `domain/item_correction.rs` 160, `application/mod.rs` 92, tests 950 + 223 physical lines; executable nesting depth ≤3; cyclomatic complexity `N/A — no configured complexity tool`. All units below review thresholds; no exception recorded or needed. |
| A-7 | Contract and baseline risks covered | EP-01 through EP-08 and affected inherited clauses map to checks; exactly five risk dimensions assessed and applicable rows reach Pass. | Traceability, risk/state matrices, and check results. | Complete | 2026-09-24: EP-01 through EP-08 and the inherited CR-01 through CR-05, CA-03/CA-05/CA-09 rows map to executed checks. AC-1 through AC-6 are Pass; the three applicable risk dimensions (concurrency and ordering, resource bounds and backpressure, framework or trust-boundary rejection) are Pass with recorded evidence; the two N/A dimensions carry the completed boundary audit. AC-7's delivery-gate remainder stays with the record-level blocker and does not reopen a mapped clause. |
| A-8 | Governance validation passed | Both routed governance commands succeed for the document state under review. | Command output and source/document revision. | Complete | On 2026-09-24, after acceptance and the Clause ID row split, pre-evidence ADR blob `2ccd26f85ffcf318f02bd6dd83b3f653a5dea6bf`, ADD blob `6cb403ccf2c22595355b13c2709f77764365c63b`, and index blob `d31f33b67163f4714e5ffb057fbc8ddf9bfd08d5` were checked: `npm test --prefix tools/governance-validator` reported 208 passed, 0 failed, 0 skipped; `npm run validate --prefix tools/governance-validator` passed; `git diff --check` passed. The ADR blob precedes only this evidence update and is not an approval-context revision. Re-run for the implementation-stage document state on 2026-09-24 with ADR blob `df0fe7c7bf438a8f1bd4c78f9a70694d742253cd` (pre-evidence of this row only), ADD blob `04febab9d3ae1a7f1b62dc8a3e0653a7de331ebc`, and index blob `79c4675bb45ff695d5826d6225a2433c3177ae99`: `npm test --prefix tools/governance-validator` reported 208 passed, 0 failed, 0 skipped; `npm run validate --prefix tools/governance-validator` passed; `git diff --check` passed. |

## Supporting Notes [Optional]

Drafting selected the Current ADD's existing CAND-12 outcome without changing
its scope or dependencies. The source requirement is the captured ADD baseline;
the Trello card was not reread or changed. The initial 2026-09-24 user
instruction authorized drafting; the later `@linhai Approve` in this task
accepted this ADR. Implementation remains Not Started. The repository ADR
index had no non-terminal Full/Lightweight ADR before creating this record;
the existing Blocked OCR does not participate in ADR serialization.

CAND-13 must obtain real per-Turn provenance from its authenticated history
read. Today `prior_thread_items_async` orders its query by
`turns.created_at`, then `turn_items.turn_id`, then Turn-local `sequence`, but
returns only decoded `Item` values without `turn_id`. Those ordered, flattened
values cannot reconstruct each Item's actual Turn or safely acquire scope by
adding labels afterward. CAND-13 should define a provenance-preserving read
before it connects this projection to provider input.

Local structured review round 1 on 2026-09-24 examined ADR working-draft blob
`f7a7287134e4e9bc35453b9b46b4c85d1c097e2b`, ADD blob
`7b7572b200a2b430357e7f752aad7978e06832a5`, and decision-index blob
`5db0910bcf75a49da871863289e6938c325101c1`, against baseline
`288e2cccf8e0b8c3c7cdd501c490f7373b89e653`. Review checked service routing,
the single C-2 implementation boundary, existing CAND-3/CAND-11 constraints,
scope provenance, one-Item replacement, raw validation/error precedence,
post-terminal corrections, borrowed-resource ownership, exact acceptance
outcomes, five risk dimensions, requirement levels, and reciprocal lifecycle
links. Result: no unresolved finding in this local document review. The
subsequent checklist and review-log additions record evidence only; these
pre-evidence blobs are not an approval-context revision or pushed review
coverage.

The task reviewer then checked template, governance, source symbols, accepted
clauses, and both validator commands and reported no blocking issue. Local
structured review round 2 on 2026-09-24 examined revised ADR working-draft
blob `008783b6c2b7f43231250185fdec884ee2723f38` with the same ADD and index
blobs. It verified the review dispositions: all three option descriptions,
specific reasons for unmapped inherited clauses, scope-error index without
scope values, inspectable raw-error cause, restartable iterator semantics,
precise complexity statement, consistent sync/async symbol references, and
the CAND-13 provenance road map against the SQL query. The query orders by
created time, Turn ID, and Item sequence; its return type omits Turn ID. Result:
no unresolved finding in this local follow-up review. Two agent-driven local
review rounds have been used; the task reviewer's separate review is not
counted against that limit. The blob identifies the content before these
evidence-only checklist and review-log updates, not formal approval or pushed
automatic-review coverage.

Both governance commands passed (208 validator tests, no failures or skips;
repository validation passed), and the whitespace check passed. Only this
ADR, the ADD lifecycle/link entry, and the central decision index changed.
Per the documentation-only workflow, Rust/PostgreSQL/Sonar implementation
acceptance was not run for this draft; AC-1 through AC-7 remain Not Started.
No source, runtime configuration, test implementation, commit, push, or external
coordination change is part of this drafting task. An implementation PR's
configured automatic-review and CI gates remain future delivery requirements.

The subsequent task review confirmed no blocking defect and identified an
acceptance-evidence gap: Cargo can exit 0 with zero tests matching `--exact`.
This revision adds an explicit one-test count to each AC-1 through AC-6 result
and evidence field, records that CA-01/CA-06 content validation is not
repeated in EP-04, and maps EP-01's future caller prohibition to the AC-7
no-production-caller inspection. It is a bounded response to that review; no
new agent-driven review round or implementation acceptance is claimed.

Implementation stage, 2026-09-24: the CAND-12 slice was delivered on task
branch `codex/cand-12-effective-correction-projection` test-first — the
one-root/one-correction substitution case failed with unresolved imports
(E0432) before the production change, then passed after the projection module,
exports, and raw-validator seam landed. Routed Rust and governance commands
and the complete disposable-database test suite are green (AC-7 evidence). The
Sonar command remains blocked by the pinned coverage-selection gap recorded in
the metadata; resolving it is an owner decision that this ADR's EP-08 scope
and the repository-wide serialization gate deliberately leave outside this
task. No additional agent-driven document-review round is claimed for this
implementation stage; the acceptance checks and their recorded outputs are
the verification evidence, and push, required-CI confirmation, and
automatic-review coverage remain future delivery requirements.

## Archival [Conditionally Required — Decision Status is `Rejected`, or Decision Status is `Deprecated` or `Superseded` and Implementation Status is final]

Inactive future-lifecycle guidance: when the canonical archival trigger applies,
move this record to
`koduck-ai/docs/adr/archive/ADR-0005-effective-correction-projection.md`,
update every marker and reciprocal ADD/ADR reference, retain its central index
row with the new path and final statuses, and maintain reciprocal supersession
paths when applicable. No retirement or archival has been authorized.

## Change Log [Required]

| Date | Change | Author |
| --- | --- | --- |
| 2026-09-24 | Drafted the service Full ADR for ADD-0001 CAND-12 at source baseline `288e2cccf8e0b8c3c7cdd501c490f7373b89e653`; selected the candidate and added the reciprocal central-index entry. Proposed scoped borrowed projection, exact chain-tip semantics, fail-closed rejection, and predetermined verification; no implementation or approval claimed. | @codex |
| 2026-09-24 | Recorded local structured review round 1 and passing draft governance validation: 208 validator tests, repository validation, and whitespace check. Review found no unresolved issue; recorded draft-stage checklist evidence while retaining Proposed/Not Started and unexecuted implementation checks. | @codex |
| 2026-09-24 | Addressed the task reviewer's nonblocking suggestions: completed option descriptions and inherited-clause dispositions; made scope-error index, inner raw-error cause, iterator replay, and complexity wording precise; aligned sync/async symbol references and recorded the CAND-13 provenance gap from the actual SQL query. Rechecked revised acceptance criteria in local review round 2; no unresolved finding. Decision and implementation statuses remain Proposed/Not Started. | @codex |
| 2026-09-24 | Addressed the subsequent task review's nonblocking acceptance clarification: AC-1 through AC-6 now each require proof that exactly one named test ran, EP-04 leaves admission content validation with CA-01/CA-06, and EP-01 maps the future no-relabeling rule to AC-7's call-site inspection. No new agent-driven review round or implementation result is claimed. | @codex |
| 2026-09-24 | Recorded the repository owner's `@linhai Approve` in this ADR-0005 task at `2026-09-24T10:01:41Z`; moved Decision Status to Accepted while Implementation Status remains Not Started, and synchronized the central index and CAND-12 lifecycle evidence. No implementation result is claimed. | @codex |
| 2026-09-24 | After acceptance activated the governance validator's Clause ID check, expanded the grouped inherited CR/CA traceability entries into individual rows without changing their contract text or check mapping. | @codex |
| 2026-09-24 | Delivered T-1 test-first on task branch `codex/cand-12-effective-correction-projection`: new `application/correction_projection.rs` with the EP-01 through EP-08 pure scoped projection, behavior-preserving `validate_raw_replay_refs` seam in `domain/item_correction.rs`, module exports, and focused fixtures in `tests/cand_12_projection.rs` plus its child fixture module. Routed fmt, Clippy, full disposable-database test suite, and both governance commands are green; AC-1 through AC-6 are Pass and the three applicable risk rows reach Pass. | @codex |
| 2026-09-24 | Recorded the AC-7 coverage blocker: the pinned `rust-coverage.sh` selection cannot cover the new module (probe measured 105 executable lines / 0 covered for `correction_projection.rs`, ≈38% changed-line coverage), the selection update is outside this ADR's EP-08 scope, and the serialization gate forbids drafting the governing record while this ADR is non-terminal. Moved Implementation Status from `Not Started` to `Blocked` from `Not Started` with blocker owner `@linhai` and the recorded exit criterion; checklist A-2 In Progress, A-6/A-7 Complete. | @codex |
