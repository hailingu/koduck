# ADR-0005: Provider Delta Coalescing And 512-Item Turn Budget

## Metadata [Required]

- **Decision Status**: Accepted
- **Implementation Status**: Complete
- **Date**: 2026-08-26
- **Author**: @codex
- **Decision Owner**: @linhai
- **Required Approver**: @linhai
- **Record Scope**: Project
- **Approver [Conditionally Required — Decision Status is or has been `Accepted`]**: @linhai
- **Approval Time [Conditionally Required — Decision Status is or has been `Accepted`]**: 2026-08-26T14:21:40Z
- **Approval Evidence [Conditionally Required — Decision Status is or has been `Accepted`]**: Approve
- **Rejector [Conditionally Required — Decision Status is `Rejected`]**: N/A — Decision Status is `Accepted`
- **Rejection Time [Conditionally Required — Decision Status is `Rejected`]**: N/A — Decision Status is `Accepted`
- **Rejection Evidence [Conditionally Required — Decision Status is `Rejected`]**: N/A — Decision Status is `Accepted`
- **Retired By [Conditionally Required — Decision Status is `Deprecated` or `Superseded`]**: N/A — Decision Status is `Accepted`
- **Retirement Time [Conditionally Required — Decision Status is `Deprecated` or `Superseded`]**: N/A — Decision Status is `Accepted`
- **Retirement Evidence [Conditionally Required — Decision Status is `Deprecated` or `Superseded`]**: N/A — Decision Status is `Accepted`
- **Retirement Reason [Conditionally Required — Decision Status is `Deprecated` or `Superseded`]**: N/A — Decision Status is `Accepted`
- **Blocked From [Conditionally Required — Implementation Status is `Blocked`]**: N/A — Implementation Status is `Not Started`
- **Blocker And Evidence [Conditionally Required — Implementation Status is `Blocked`]**: N/A — Implementation Status is `Not Started`
- **Blocker Owner [Conditionally Required — Implementation Status is `Blocked`]**: N/A — Implementation Status is `Not Started`
- **Blocker Exit Or Recheck Criterion [Conditionally Required — Implementation Status is `Blocked`]**: N/A — Implementation Status is `Not Started`
- **Related [Optional]**: `docs/adr/ADR-0001-provider-neutral-turn-kernel.md`; `docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md`; `docs/adr/ADR-0004-provider-stream-completion-normalization.md`
- **Architecture Source [Conditionally Required — product demand]**: N/A — this corrective provider-fragmentation compatibility task was discovered through local verification of the existing provider-neutral runtime and is not derived from a new Trello product requirement
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

The provider-neutral Turn kernel currently treats every non-empty content
fragment from an OpenAI-compatible stream as one canonical
`agent_message_delta` Item. The application durably appends that Item and only
then publishes its REST/SSE representation. ADR-0001 also caps the shared
post-acceptance provider, Tool-projection, usage, and terminal budget at 64
Items and 1 MiB, reserving room for a mandatory terminal.

On 2026-08-26, a local request through the configured MiniMax M3 provider
returned valid ordered content in many small fragments. PostgreSQL accepted the
Thread, Turn, lease, user message, and 63 `agent_message_delta` Items. The next
fragment exhausted the 64-Item post-acceptance budget, so the kernel appended a
durable `failed` terminal with code `DURABILITY_UNAVAILABLE`, stopped provider
consumption, and the synchronous route returned `503 durability-unavailable`.
Database inspection showed an accepting PostgreSQL server, no blocked lock,
and the complete durable prefix, proving that provider wire fragmentation—not
storage unavailability—caused the failure.

Provider chunk boundaries are transport artifacts: equivalent text may arrive
as one fragment or hundreds. Making each fragment a canonical Item lets a
provider-controlled framing choice consume a Turn-level durability budget and
causes ordinary responses to fail before their semantic content or serialized
byte total approaches the accepted 1-MiB bound. Raising the count alone reduces
the immediate failure rate but leaves correctness dependent on arbitrary wire
fragmentation. The complete corrective task must therefore normalize raw
fragments into bounded durable Items and raise the post-acceptance Item budget
from 64 to 512 while retaining append-before-publish, byte bounds, interruption,
Tool, recovery, and provider-neutral semantics.

**Requirement baseline RQ-1**: the repository-owner instruction received in
this task on 2026-08-26 states: “64个 Item 太小了，改为512个”. This recorded
baseline is the durable source for the exact 512-Item requirement; no external
task system or Trello product requirement is being inferred.

## Scope [Required]

In scope:

- Introduce one application-owned delta coalescer between provider events and
  durable `agent_message_delta` Items.
- Raise the shared post-acceptance Turn budget from 64 to exactly 512 Items,
  including coalesced agent deltas, usage, Tool projections, and the terminal;
  the initial durable user message remains outside this existing accounting
  scope.
- Flush coalesced content at exact byte, latency, ordering, interruption, and
  terminal boundaries while preserving byte-for-byte UTF-8 content order.
- Resize the bounded HTTP/SSE delivery channel for the 512-Item contract without
  removing backpressure or cancellation behavior.
- Distinguish count or payload exhaustion from a storage outage through the
  durable terminal code `RESOURCE_LIMIT_EXCEEDED` and synchronous problem code
  `resource-limit-exceeded`, while retaining the existing SSE terminal shape.
- Update project and service contract copies, comments, focused regressions,
  and routed verification for the changed resource contract.

Out of scope:

- Filtering, hiding, parsing, or assigning semantics to provider content marked
  with `<think>`, reasoning, analysis, or any other provider-specific tag; such
  content remains ordinary text and is preserved exactly.
- Increasing the 1-MiB per-Turn serialized payload bound, 4096-Item resume
  context bound, 32 assembled Tool-call bound, Tool input/output byte bounds,
  provider deadlines, PostgreSQL deadlines, worker admission, or request input
  limit.
- Changing the PostgreSQL schema, historical Items, Thread ownership, lease
  fencing, correction semantics, Tool authorization, provider selection,
  deployment, dependencies, or runtime environment variables.
- Publishing raw provider fragments before their coalesced content has been
  durably appended.

## Tensions, Constraints, And Open Questions [Required]

### Identified Tensions [Conditionally Required — competing goals or trade-offs exist]

| ID | Tension | Impact | Decision |
| --- | --- | --- | --- |
| TN-1 | Low streaming latency versus bounded durable Item count | Flushing every fragment preserves provider timing but allows arbitrary fragmentation to exhaust the budget; buffering until completion removes useful streaming | Flush at 16,384 buffered UTF-8 bytes or 500 ms after the first buffered byte, whichever occurs first, and flush before every semantic boundary |
| TN-2 | Larger valid Turns versus bounded memory, storage, and SSE backpressure | Raising the count without preserving byte and channel limits can multiply memory and delivery pressure | Raise only the post-acceptance Item count to 512; retain the 1-MiB serialized payload cap and use a channel capacity derived from the new exact event maximum |
| TN-3 | Clear failure diagnosis versus wire compatibility | Reusing `durability-unavailable` hides resource exhaustion; changing terminal JSON fields breaks the exact v1 schema | Use a distinct durable failure code and synchronous problem code, but retain the existing `turn.failed` SSE document shape and append-before-publish ordering |
| TN-4 | Provider-neutral content preservation versus reasoning suppression | Tag-based filtering could remove user-visible model output or depend on mutable vendor conventions | Preserve all provider content byte-for-byte; reasoning-control policy requires a separate accepted decision |

### Constraints [Required]

- ADR-0001 remains authoritative for provider-neutral ownership, ordered
  append-before-publish, the 1-MiB payload cap, two-second PostgreSQL attempts,
  fenced liveness, replay, and the v1 REST/SSE shape except where PLB-1 through
  PLB-9 below explicitly replace its 64-Item and resource-limit behavior.
- ADR-0003 remains authoritative for Tool-call assembly, projection ordering,
  authorization, execution, and the exact maximum of 32 assembled Tool calls.
- ADR-0004 remains authoritative for `[DONE]`, explicit clean-end, finish,
  timeout, cancellation, and malformed-frame terminal semantics.
- Raw provider content order and bytes must be preserved exactly after
  concatenating the resulting durable delta contents; the coalescer must never
  trim, normalize, interpret, or reorder text.
- The coalescer must hold at most 16,384 buffered content bytes. An incoming
  fragment crossing that bound must be split only at a valid UTF-8 scalar
  boundary and must preserve exact concatenation.
- A non-empty accumulator must be flushed no later than 500 ms after its first
  buffered byte under an advancing runtime clock. The timer is measured before
  the bounded durable append; the existing append deadline remains separate.
- Coalesced content must flush before usage, Tool-call delivery, provider
  completion or error, Tool-round continuation, authenticated interruption,
  dependency cancellation, disconnect cancellation, or any other Turn terminal.
- Every visible coalesced Item must be durably appended first. If its append
  fails, no byte from that uncommitted Item may be published.
- Every admitted nonterminal must leave one slot for the mandatory terminal.
  Item 512 is legal only as that final terminal, and Item 513 is always rejected
  before append or publication. The initial user message is not counted,
  matching the existing post-acceptance accounting boundary.
- The 1-MiB canonical serialized payload cap remains jointly enforced with the
  512-Item cap; satisfying one limit does not waive the other.
- Actual history I/O, commit-acknowledgement, control-read, or append-deadline
  failures retain `durability-unavailable`. Count or payload exhaustion must
  append `RESOURCE_LIMIT_EXCEEDED` and map synchronous delivery to
  `422 resource-limit-exceeded`; after SSE start, the existing exact
  `turn.failed` terminal document remains unchanged.
- No new dependency, persisted schema, runtime variable, or external operation
  is authorized by this record.
- Every maintained source file changed by implementation must cite
  `docs/adr/ADR-0005-provider-delta-coalescing-and-512-item-turn-budget.md` at
  its first legal comment position while retaining applicable existing markers.
- Source implementation must follow Red-Green-Refactor after this record is
  accepted; ADR drafting and governance validation are documentation-only.

### Open Questions [Conditionally Required — material questions exist or were resolved during drafting]

| ID | Question | Owner | Due | Status | Resolution and Evidence |
| --- | --- | --- | --- | --- | --- |
| Q-1 | What exact Item limit replaces 64? | @linhai | 2026-08-26 | Resolved | Requirement baseline RQ-1 records the repository-owner instruction requiring 512; PLB-5 defines its exact accounting scope and boundary behavior. |
| Q-2 | Is increasing the count alone sufficient? | @linhai | 2026-08-26 | Resolved | No. The 2026-08-26 durable trace shows arbitrary provider chunk boundaries consumed the old budget; PLB-1 and PLB-2 require normalization before persistence. |
| Q-3 | Should `<think>` or reasoning-tagged content be removed in this task? | @linhai | 2026-08-26 | Resolved | No. This task preserves provider content exactly. Filtering changes content semantics and requires a separate accepted decision. |
| Q-4 | Must a resource-limit failure remain indistinguishable from a database outage? | @linhai | 2026-08-26 | Resolved | No. PLB-7 introduces distinct durable and synchronous diagnostics while preserving the exact SSE terminal document. |
| Q-5 | What HTTP status should synchronous resource exhaustion use? | @linhai | 2026-08-26 | Resolved | Use `422`: the request syntax and media type are valid, but its Turn cannot be processed within the deterministic output budget. `413` describes oversized request content, `429` describes request rate, `507` implies insufficient storage, and `503` implies temporary service unavailability; each would misstate this failure. |

## Decision Drivers [Required]

1. **Fragmentation independence**: Semantically identical provider text must not
   succeed or fail solely because its wire chunks are differently sized.
2. **Durable-before-visible ordering**: No content may reach a client before its
   canonical coalesced Item is durable.
3. **Exact resource control**: The 512-Item and 1-MiB limits, timer, buffer, and
   channel bounds must be explicit and deterministically testable.
4. **Streaming usefulness**: A client must receive bounded incremental output
   rather than waiting for an entire provider response.
5. **Provider neutrality**: Coalescing must use owned time and byte boundaries,
   not provider, hostname, model, tag, or credential identity.
6. **One reviewable slice**: The application coalescing policy and its limited
   transport/history support must fit one implementation pull request.

## Options Considered [Required]

### Option: Raise the Item limit to 512 without coalescing

Change the count constant and matching channel/tests while retaining one Item
per raw provider delta.

Pros:

- Small implementation diff.
- Allows the observed response to complete if it stays below 511
  post-acceptance nonterminal Items plus its reserved terminal.

Cons:

- Success still depends on provider-controlled chunk granularity.
- A long or slowly fragmented ordinary response can still exhaust 512 Items far
  below the 1-MiB content bound.
- Multiplies database writes and SSE events without adding semantic value.

### Option: Coalesce provider deltas and raise the Item limit to 512

Buffer raw content in the application boundary, flush exact coalesced Items at
16,384 bytes, 500 ms, or semantic boundaries, and raise the shared
post-acceptance Item budget and delivery capacity to 512.

Pros:

- Makes durable Item creation substantially independent of raw provider frame
  size while retaining incremental delivery.
- Preserves append-before-publish and the existing Item payload schema.
- Provides sufficient bounded capacity for long responses and Tool projections
  without increasing the 1-MiB byte allowance.

Cons:

- Adds an ordering-sensitive accumulator and timer to Turn execution.
- May add up to 500 ms before the first or next visible text Item.
- Changes the number and boundaries of `agent_message_delta` Items observed by
  clients, although concatenated content and wire fields remain unchanged.

### Option: Publish raw deltas while periodically rewriting one durable Item

Expose every provider fragment immediately and update or replace a persisted
assistant Item as content grows.

Pros:

- Preserves provider-level streaming granularity with few final rows.

Cons:

- Violates append-before-publish and append-only history.
- Creates mutable identity, replay, crash-consistency, and correction semantics
  outside this task's safe boundary.
- Requires schema and protocol changes across multiple implementation slices.

## Decision [Required]

**Selected option**: Coalesce provider deltas and raise the Item limit to 512.

**Rationale**: The selected option removes raw frame count as the immediate
durability boundary, preserves exact content and durable-before-visible
ordering, and keeps every memory, time, payload, and delivery queue bound
explicit. The 512 count supplies headroom for long responses and Tool
projections, while the unchanged 1-MiB cap prevents count growth from becoming
unbounded memory or storage growth. The change stays centered on one
application orchestration policy with only the adjacent channel, diagnostic,
contract-copy, and test changes required to expose that policy correctly.

### Provider-Level Buffer Contract [Required]

- **PLB-1 — Raw fragments are not canonical Items**: A non-empty
  `ProviderEvent::Delta` contributes bytes to one application-owned accumulator.
  Raw provider event count must not increment the post-acceptance Item counter
  until a coalesced `agent_message_delta` is planned for durable append.
- **PLB-2 — Exact coalescing boundaries**: The accumulator flushes immediately
  before adding content that would exceed 16,384 UTF-8 bytes, and flushes no
  later than 500 ms after its first buffered byte. An individual fragment above
  16,384 bytes is split at UTF-8 scalar boundaries into the minimum ordered
  sequence of non-empty chunks no larger than 16,384 bytes. Concatenating every
  emitted content field must exactly reproduce the provider content.
- **PLB-3 — Semantic-boundary flush**: Buffered content flushes before usage,
  Tool-call delivery, provider completion/error, Tool-round continuation,
  authenticated interruption, dependency/disconnect cancellation, or any
  terminal append. Empty accumulators emit no Item.
- **PLB-4 — Durable publication**: Each coalesced Item is published only after
  its successful durable append. Append failure publishes none of that Item's
  content and enters the existing bounded durability recovery path.
- **PLB-5 — Exact 512-Item budget**: The shared post-acceptance count budget is
  exactly 512 Items and includes coalesced agent deltas, usage, approval/tool
  projections, and the terminal. The initial user message remains excluded.
  Items 1 through 511 may be nonterminal only while one terminal slot remains;
  Item 512 is legal only as the final mandatory terminal, and Item 513 is
  rejected before append or publication.
- **PLB-6 — Independent byte bound**: The existing 1,048,576-byte cumulative
  canonical serialized payload limit is unchanged and is evaluated against
  coalesced Items plus all other counted Items. Coalescing never grants extra
  bytes or truncates content.
- **PLB-7 — Resource diagnostics**: Count or payload rejection durably closes
  the Turn as `failed` with code `RESOURCE_LIMIT_EXCEEDED`. Synchronous delivery
  returns `422 resource-limit-exceeded`. A started SSE stream emits the existing
  exact durable `turn.failed` terminal and no contradictory `error` event.
  Actual history unavailability and append-deadline failures retain
  `durability-unavailable` and its existing recovery/transport behavior.
- **PLB-8 — Backpressure and retained limits**: The runtime's bounded SSE queue
  must admit one `turn.started`, at most 512 counted Item/terminal publications,
  and one possible in-band error without treating legal output as downstream
  cancellation. The 32 Tool-call, 4096 history-Item, 1-MiB history, provider
  5/30/30/120-second, PostgreSQL two-second, and 256-worker limits remain exact.
- **PLB-9 — Provider-neutral content**: Coalescing must not inspect provider
  identity or text tags. Reasoning-tagged content remains ordinary content and
  must be preserved exactly unless a later Accepted ADR changes that policy.

### Consequences [Required]

Positive:

- The observed high-fragment valid response can complete without approaching
  either the 512-Item or 1-MiB bound.
- Database write and SSE event amplification from tiny provider frames is
  reduced while clients retain bounded incremental output.
- Resource exhaustion becomes distinguishable from an actual durability outage
  in durable and synchronous diagnostics.
- Existing rows and payload schemas remain replay-compatible.

Negative:

- Visible delta boundaries change and streaming may wait up to 500 ms for a
  partial coalesced Item.
- The application runner gains timer and accumulator state that must arbitrate
  correctly with Tool rounds, interruption, cancellation, and terminal events.
- Legal worst-case Turn metadata and channel capacity grow from 64 to 512
  counted Items even though payload bytes remain capped.

Mitigations:

- Keep the accumulator byte- and time-bounded and test every flush boundary
  with a deterministic clock and UTF-8 edge fixtures.
- Reuse the existing history append and terminal arbitration paths instead of
  adding mutable persistence or a second publication mechanism.
- Keep all unrelated action, context, payload, deadline, and worker bounds
  unchanged and test exact-at-limit plus one-over cases.

## Implementation Plan [Required]

**Complete task outcome**: One independently reviewable implementation pull
request makes provider wire fragmentation independent of canonical Item
creation through the PLB coalescing contract, accepts exactly 512 shared
post-acceptance Items while retaining the 1-MiB cap and terminal reservation,
completes the reproduced high-fragment response with exact concatenated content,
and preserves deterministic ordering, interruption, recovery, backpressure,
and retained limits.

**Primary implementation boundary**: Application Turn orchestration and
durability policy — `koduck_ai::application::runner` and
`koduck_ai::application::durability`.

Allowed subtask statuses: `Not Started`, `In Progress`, `Blocked`, `Complete`,
or `N/A — <specific reason>`.

| ID | Objective or deliverable | Included scope | Status | Actual implementation evidence |
| --- | --- | --- | --- | --- |
| T-1 | Implement the coalescer and exact 512-Item shared Turn budget test-first | Application accumulator/timer, UTF-8 splitting, semantic flush ordering, terminal reservation, resource failure typing, Tool-projection accounting, and focused unit/integration tests | Complete | `koduck-ai/src/application/delta_coalescer.rs` (new, 120 lines: `DeltaCoalescer::push`/`take_due_flush`/`take_forced_flush` with UTF-8 `scalar_boundary` splitting); `koduck-ai/src/application/runner.rs` (Delta events buffer into the accumulator, Pending events apply the latency flush, every semantic/terminal boundary flushes through `runner_terminals::flush_buffered_deltas`/`append_coalesced_deltas` before the boundary item); `koduck-ai/src/application/durability.rs` (`max_items` 64→512, per-variant `problem_code`, `RESOURCE_LIMIT_TERMINAL_CODE`); `koduck-ai/src/application/ports.rs` (`TurnRunError::ResourceLimit`/`ResourceLimitFailure`); `koduck-ai/src/application/runner/failure.rs` (`terminalize_from_limit` appends the resource terminal and surfaces the typed failure). Observed RED first: `cargo test -p koduck-ai --test cand_1_durability --no-run` failed with `E0432 unresolved import DeltaCoalescer/ResourceLimitFailure` and `E0599 no variant ResourceLimit`; after implementation AC-1, AC-2, AC-4, and AC-6 pass. |
| T-2 | Align adjacent delivery capacity, public diagnostics, contract copies, and full regression evidence | Axum SSE queue capacity, synchronous problem mapping, REST/SSE contract copy, CAND-2 limit wording, exact fixtures where delta grouping changes, production-boundary regressions, and routed checks | Complete | `koduck-ai/src/runtime/mod.rs` (`pub const STREAM_BUFFER_CAPACITY = 514`); `koduck-ai/src/adapters/http/mod.rs` (`ServiceError::ResourceLimitExceeded` → `422 resource-limit-exceeded`); `koduck-ai/src/adapters/provider/stream_state.rs` and `koduck-ai/src/application/tool_projection.rs` (retained-limit wording); contract copies `koduck-ai/docs/contracts/cand-1-rest-sse-v1.md` (coalesced delivery grouping, `422 resource-limit-exceeded`) and `cand-2-tool-approval-v1.md` (512-Item shared allowance, retained 32-call bound); budget regressions in `koduck-ai/src/adapters/history/postgres/sqlx_executor/{recovery_budget,interruption_ownership}.rs` updated to the 512 boundary; delta-grouping fixtures updated across `tests/cand_1_kernel.rs`, `tests/cand_1_durability.rs`, `tests/cand_1_contract.rs`, `tests/cand_1_liveness.rs`, `tests/cand_2_runner_projection_guards*.rs`, and `tests/runtime_wiring.rs`. AC-5, AC-7, and AC-8 pass with the routed command evidence below. |

**Affected paths**: `docs/adr/INDEX.md`;
`docs/adr/ADR-0005-provider-delta-coalescing-and-512-item-turn-budget.md`;
`koduck-ai/src/application/delta_coalescer.rs` (new);
`koduck-ai/src/application/mod.rs`;
`koduck-ai/src/application/durability.rs`;
`koduck-ai/src/application/runner.rs`;
`koduck-ai/src/application/runner/failure.rs`;
`koduck-ai/src/application/runner/tool_call.rs` (new);
`koduck-ai/src/application/runner_terminals.rs`;
`koduck-ai/src/application/ports.rs`;
`koduck-ai/src/application/tool_projection.rs`;
`koduck-ai/src/adapters/http/mod.rs`;
`koduck-ai/src/adapters/provider/stream_state.rs`;
`koduck-ai/src/adapters/history/postgres/sqlx_executor/recovery_budget.rs`;
`koduck-ai/src/adapters/history/postgres/sqlx_executor/interruption_ownership.rs`;
`koduck-ai/src/runtime/mod.rs`;
`koduck-ai/docs/contracts/cand-1-rest-sse-v1.md`;
`koduck-ai/docs/contracts/cand-2-tool-approval-v1.md`;
`koduck-ai/tests/cand_1_contract.rs`;
`koduck-ai/tests/cand_1_durability.rs`;
`koduck-ai/tests/cand_1_kernel.rs`;
`koduck-ai/tests/cand_1_liveness.rs`;
`koduck-ai/tests/cand_2_runner_projection_guards.rs`;
`koduck-ai/tests/cand_2_runner_projection_guards/lifecycle_guards.rs`;
`koduck-ai/tests/provider_stream_lifecycle.rs`;
`koduck-ai/tests/runtime_wiring.rs`;
and changed `koduck-ai/tests/fixtures/*` golden files when deterministic
coalescing changes their exact Item grouping (none required: the v1 golden
fixtures replay prebuilt single-delta results whose grouping is unchanged).
The two `sqlx_executor` files and `cand_1_kernel.rs` entries are the
in-scope focused regressions of the changed resource contract (Scope:
"Update … focused regressions"), added here as evidence-only enumeration
after implementation; the two new modules decompose the accumulator and the
Tool-call seam out of `runner.rs`.

### Stable Implementation Touchpoints [Conditionally Required — source or configuration implementation]

| Path | Stable symbol or contract anchor | Key code excerpt, when needed | Purpose | Source revision |
| --- | --- | --- | --- | --- |
| `koduck-ai/src/application/runner.rs` | `koduck_ai::application::runner::ExecutionState`; `handle_event`; `enforce_provider_limits`; `terminalize_from_limit` | N/A — stable symbols identify the one-to-one delta append, resource accounting, and terminal conversion | Primary orchestration seam where raw deltas become canonical Items and race with semantic/terminal boundaries | `023ee95a7e0de19e140866dfe487fcfbd90b1958` |
| `koduck-ai/src/application/durability.rs` | `koduck_ai::application::AppendPolicy::cand_1`; `BufferLimitError`; `reserve_durability_terminal` | N/A — stable symbols identify exact count, byte, and reserved-terminal policy | Owns the 64-to-512 count decision while retaining the independent 1-MiB cap | `023ee95a7e0de19e140866dfe487fcfbd90b1958` |
| `koduck-ai/src/application/runner/failure.rs` | `validate_provider_item`; `history_failure` | N/A — stable symbols identify post-acceptance accounting and durability-error conversion | Separates counted durable Items and resource exhaustion from true history unavailability | `023ee95a7e0de19e140866dfe487fcfbd90b1958` |
| `koduck-ai/src/application/tool_projection.rs` | `koduck_ai::application::tool_projection::TurnProjectionSink` | N/A — stable type identifies the shared provider/Tool projection budget | Preserves one 512-Item/1-MiB allowance across coalesced output and Tool projections | `023ee95a7e0de19e140866dfe487fcfbd90b1958` |
| `koduck-ai/src/runtime/mod.rs` | `STREAM_BUFFER_CAPACITY`; `handle_stream_request` | N/A — stable constant and function identify bounded SSE delivery | Aligns queue capacity and disconnect backpressure with the exact legal event maximum | `023ee95a7e0de19e140866dfe487fcfbd90b1958` |
| `docs/adr/ADR-0001-provider-neutral-turn-kernel.md` | `CT-8`; `CT-9`; `AC-9` | N/A — stable contract clause and check anchors are sufficient | Historical 64-Item and durability mapping baseline explicitly amended by PLB-1 through PLB-8 | `023ee95a7e0de19e140866dfe487fcfbd90b1958` |

**Migration and rollback strategy [Conditionally Required — this replaces or
changes existing behavior]**: Forward migration changes no schema and rewrites
no historical Item. New Turns use coalesced deltas and the 512-Item policy;
existing per-delta and coalesced Items remain valid because both use the same
append-only `agent_message_delta` payload. Stop before rollout if any PLB check,
full routed suite, or contract fixture fails. Rollback reverts the source and
contract-copy changes; already completed Turns with more than 64
post-acceptance Items remain replayable under the unchanged 4096-Item/1-MiB
history context limits, while new Turns return to the old admission policy. No
data rollback or destructive operation is required.

### Engineering Exceptions [Conditionally Required — an engineering rule is exceeded or waived]

N/A — no engineering exception is proposed. Implementation must decompose any
new accumulator into a cohesive unit and record a decomposition review if an
affected source or test unit crosses its configured review threshold.

## Contract-To-Check Traceability [Conditionally Required — source or configuration implementation]

| Clause ID | Authoritative contract path and heading | Exact normative requirement | Acceptance check or deterministic test IDs | Explicit coverage method |
| --- | --- | --- | --- | --- |
| PLB-1 | `docs/adr/ADR-0005-provider-delta-coalescing-and-512-item-turn-budget.md` — Provider-Level Buffer Contract | Raw provider delta count does not increment the Item budget until one coalesced Item is planned | AC-1 | Feed 640 one-byte raw deltas and inspect durable append count, exact concatenation, and completion |
| PLB-2 | Same path — Provider-Level Buffer Contract | Flush at 16,384 UTF-8 bytes or 500 ms and split only at valid UTF-8 boundaries | AC-2 | Exercise exact/one-over byte cases and paused-time 499/500-ms cases with multibyte text |
| PLB-3 | Same path — Provider-Level Buffer Contract | Flush pending text before every semantic, Tool, cancellation, interruption, or terminal boundary | AC-3, AC-6 | Script each boundary and assert exact durable order and no empty Item |
| PLB-4 | Same path — Provider-Level Buffer Contract | Publish only after successful append and publish no uncommitted coalesced bytes | AC-3, AC-6 | Compare append/publication traces and inject append failure at flush |
| PLB-5 | Same path — Provider-Level Buffer Contract | Exactly 512 shared post-acceptance Items are allowed with one terminal slot reserved; Item 513 is rejected | AC-4 | Exercise 511 nonterminal-plus-terminal, illegal terminal starvation, and one-over cases |
| PLB-6 | Same path — Provider-Level Buffer Contract | The 1,048,576-byte serialized payload cap remains independent and exact | AC-4 | Exercise exact-byte and one-byte-over cases under the raised count limit |
| PLB-7 | Same path — Provider-Level Buffer Contract | Resource exhaustion and actual durability outage produce the declared distinct durable and synchronous diagnostics without changing SSE terminal shape | AC-5 | Run paired resource-limit and history-outage requests through runner and HTTP/SSE adapters |
| PLB-8 | Same path — Provider-Level Buffer Contract | SSE capacity admits the legal maximum while all enumerated unrelated bounds remain unchanged | AC-7, AC-8 | Exercise exact-capacity slow-consumer delivery and run existing limit suites |
| PLB-9 | Same path — Provider-Level Buffer Contract | Coalescing is provider- and tag-neutral and preserves reasoning-tagged text exactly | AC-1, AC-8 | Include tagged content in fragmentation fixtures and inspect implementation for identity/tag branches |

## Risk Coverage Matrix [Conditionally Required — source or configuration implementation]

| Risk dimension | Applicability and scenario, or specific N/A reason | Owning boundary | Deterministic verification method | Exact expected result | Acceptance check IDs | Status | Actual evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Concurrency and ordering | Applicable — the 500-ms flush races with provider completion, Tool continuation, interrupt, and terminal writers | Application runner and PostgreSQL terminal arbitration | AC-3 and AC-6 scripted ordering/race tests | One ordered durable prefix, no duplicate/empty delta, exactly one terminal, and no publication before append | AC-3, AC-6 | Pass | 2026-08-26: AC-3 and AC-6 ordering/race fixtures green; every scenario produced one ordered durable prefix, no duplicate or empty delta, exactly one terminal, and no publication before append. |
| Timeout and deadline | Applicable — the coalescing timer, two-second append deadline, and provider idle/total deadlines interact | Application timer, history append, and provider transport | AC-2 plus existing deadline suites in AC-8 | Flush becomes eligible at exactly 500 ms; append remains capped at two seconds; provider 5/30/30/120-second outcomes remain unchanged | AC-2, AC-8 | Pass | 2026-08-26: AC-2 paused-clock cases green (flush eligible at exactly 500 ms); the routed suite retains the existing 2-second append and 5/30/30/120-second provider deadline tests, all green. |
| Cancellation and interruption | Applicable — cancellation or authenticated interruption arrives with buffered text | Application runner, HTTP disconnect signal, and PostgreSQL interrupt arbitration | AC-6 cancellation/interruption fixtures | Buffered text is either durably flushed before the winning terminal or remains entirely unpublished after append failure; one terminal wins | AC-6 | Pass | 2026-08-26: AC-6 interruption/cancellation/timer-race cases and the AC-7 disconnect case green; buffered text flushes before the winning terminal and no text follows it. |
| Resource bounds and backpressure | Applicable — exact 512/513 Items, 1-MiB/one-byte-over payload, 16-KiB accumulator, and slow SSE consumer | AppendPolicy, delta coalescer, ToolProjectionSink, and Axum queue | AC-1, AC-2, AC-4, and AC-7 boundary tests | Every exact-limit case is accepted; every one-over case is rejected before over-limit publication; legal maximum delivery is not mistaken for disconnect | AC-1, AC-2, AC-4, AC-7 | Pass | 2026-08-26: AC-1, AC-2, AC-4, and AC-7 boundary cases green; every exact-limit case commits, every one-over case closes with `RESOURCE_LIMIT_EXCEEDED` before over-limit publication, and the 514-slot queue admits the legal maximum without false cancellation. |
| Framework or trust-boundary rejection | Applicable — invalid identity/body/method requests must not allocate a coalescer or change resource diagnostics | Axum and owned HTTP adapter | Existing rejection tests plus routed inspection in AC-8 | Invalid requests retain exact 400/401/405 responses and execute zero provider/history/coalescer work; no provider/model/tag branch selects coalescing | AC-8 | Pass | 2026-08-26: existing rejection suites plus AC-8 diff inspection green; invalid requests retain the exact 400/401/405 responses with zero coalescer work and no provider/model/tag branch selects coalescing. |

## Acceptance Checks [Required]

| Check ID | Subtask | Binary acceptance point | Preconditions or input | Verification method | Exact expected result | Expected evidence | Status | Actual result and evidence |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| AC-1 | T-1 | High-fragment provider output is coalesced before persistence and completes with exact content | Deterministic provider emits 640 ordered one-byte deltas, including a literal `<think>` tag sequence, within less than 500 ms, then usage and completion | Add and run `cargo test -p koduck-ai --test cand_1_durability raw_provider_fragments_are_coalesced_before_durable_append -- --exact` | Exit 0; Turn is `completed`; concatenated durable/published content exactly equals all 640 bytes; exactly one durable agent delta exists and total counted Items are at most 512; no `durability-unavailable` occurs | Command output plus append/publication trace | Pass | 2026-08-26: exact command exited 0 in `tests/cand_1_durability.rs::raw_provider_fragments_are_coalesced_before_durable_append`; the 640-byte `<think>…` response completed with exactly one durable delta whose content equals all 640 bytes, 4 durable rows total, and no `DURABILITY_UNAVAILABLE`. |
| AC-2 | T-1 | Coalescer byte, UTF-8, and latency boundaries are exact | Paused deterministic clock; ASCII and multibyte fixtures at 16,384 bytes and one byte over; one buffered byte at 499 and 500 ms | Add and run `cargo test -p koduck-ai --test cand_1_durability delta_coalescer_flushes_at_exact_byte_and_latency_boundaries -- --exact` | Exit 0; no buffered Item exceeds 16,384 content bytes; every split is valid UTF-8; concatenation is exact; no timer flush occurs at 499 ms and exactly one eligible flush occurs at 500 ms | Command output and per-boundary trace | Pass | 2026-08-26: exact command exited 0; ASCII 16,384/16,385 splits, the three-byte-euro scalar-boundary split (16,383+3), and the 499/500/1,099/1,100-ms timer cases all pass with exact concatenation and no empty chunk. |
| AC-3 | T-1 | Coalesced Items remain durable-before-visible and precede semantic events | Script pending text followed separately by usage, Tool call, completion, provider error, and Tool-round continuation; inject one append failure | Add and run `cargo test -p koduck-ai --test cand_1_contract coalesced_deltas_preserve_semantic_order_and_append_before_publish -- --exact` | Exit 0; each successful case appends pending text first and publishes it only afterward; no empty Item exists; injected append failure publishes zero bytes from the failed Item and enters bounded recovery | Command output and ordered append/publish trace | Pass | 2026-08-26: exact command exited 0 in `tests/cand_1_contract.rs`; all five boundary scenarios order the coalesced delta durably before the semantic item, the injected first-append failure (`fail_append_at: 1`) publishes zero bytes with `consumed == 3`, and no empty delta exists. |
| AC-4 | T-1 | The shared count is exactly 512 and the payload remains exactly 1 MiB | Fixtures for 511 legal nonterminals plus terminal, attempted nonterminal that would starve the terminal, Item 513, payload 1,048,576 bytes, and payload 1,048,577 bytes across provider and Tool projections | Add and run `cargo test -p koduck-ai --test cand_1_durability turn_budget_accepts_512_and_rejects_513 -- --exact`; run `cargo test -p koduck-ai --test cand_2_runner_projection_guards` | Both commands exit 0; legal Item 512 terminal and exact-byte payload commit; terminal-starvation, Item 513, and one-byte-over cases publish no over-limit Item and close with `RESOURCE_LIMIT_EXCEEDED`; provider and Tool projections share one counter | Command output and durable replay assertions | Pass | 2026-08-26: `cargo test -p koduck-ai --test cand_1_durability turn_budget_accepts_512_and_rejects_513 -- --exact` exited 0 (512/513 policy boundary, 254-denied-call legal maximum completing at 512 counted Items, terminal starvation closing as `RESOURCE_LIMIT_EXCEEDED`, exact-byte and one-over 1-MiB payloads) and `cargo test -p koduck-ai --test cand_2_runner_projection_guards` exited 0 (28 tests, including the rewritten 512-scale atomic-preflight, reserve, payload, approval, and release fixtures). |
| AC-5 | T-2 | Resource exhaustion is distinguishable from actual durability outage without SSE schema drift | Paired synchronous and SSE requests: one exact one-over count fixture and one injected history append outage | Add and run `cargo test -p koduck-ai --test cand_1_contract resource_limit_and_durability_outage_have_distinct_diagnostics -- --exact` | Exit 0; resource case has durable `RESOURCE_LIMIT_EXCEEDED`, synchronous `422 resource-limit-exceeded`, and existing exact `turn.failed` SSE data; history outage retains `durability-unavailable` and its existing error/recovery behavior | Command output, replay terminal, and exact HTTP/SSE assertions | Pass | 2026-08-26: exact command exited 0; the resource case returns synchronous `422` with `"code":"resource-limit-exceeded"` and a started stream emits exactly one `event: turn.failed` with no `error` event, while the outage case keeps `503 durability-unavailable` and exactly one `error` event with no terminal. |
| AC-6 | T-1 | Interruption and cancellation arbitrate correctly with pending coalesced text | Paused clock with non-empty accumulator; authenticated interrupt, downstream disconnect, dependency cancellation, and a race at the 500-ms flush boundary | Add and run `cargo test -p koduck-ai --test cand_1_liveness buffered_delta_interrupt_and_cancellation_arbitration -- --exact` | Exit 0; each case has one terminal; text is durably appended before publication or wholly absent after failed append; no text appears after the winning terminal and no duplicate terminal exists | Command output and durable sequence trace | Pass | 2026-08-26: exact command exited 0; authenticated interruption, dependency/disconnect cancellation, and the 540-ms timer race each flush one coalesced delta durably before exactly one winning terminal with no text after it. |
| AC-7 | T-2 | Bounded SSE delivery admits the exact raised contract and still cancels a disconnected consumer | Slow consumer receives a legal maximum event sequence; separate consumer disconnects with pending buffered content | Add and run `cargo test -p koduck-ai --test runtime_wiring raised_turn_budget_preserves_sse_backpressure_and_disconnect -- --exact` | Exit 0; legal sequence is delivered without false cancellation or dropped event; queue capacity is exactly the declared started-plus-512-counted-plus-error bound; disconnected stream cancels and terminalizes under existing rules | Command output, queue observation, and terminal replay | Pass | 2026-08-26: exact command exited 0; `STREAM_BUFFER_CAPACITY == 514` asserted, the 509-item legal maximum (508 projections + one coalesced delta) delivered to a non-reading consumer with `turn.completed`, 509 `item.created`, no `error`, and no cancelled/interrupted terminal, and the disconnected consumer with buffered content lands the delta before the `cancelled` terminal. |
| AC-8 | T-2 | Routed implementation, retained limits, contracts, and governance all pass | Accepted implementation and updated contract copies are present; no live provider credential is required | Run `cargo fmt --all --check`; run `cargo clippy -p koduck-ai --all-targets --all-features -- -D warnings`; run `cargo test -p koduck-ai --all-targets --all-features`; run `npm test --prefix tools/governance-validator`; run `npm run validate --prefix tools/governance-validator`; inspect diff for governed-file markers, provider/model/tag branches, undeclared paths, dependencies, migrations, and runtime variables | Every command exits 0; structured inspection finds all changed maintained source markers, zero provider/model/tag-selected coalescing branch, zero new dependency/migration/runtime variable, and no retained limit drift outside PLB-1 through PLB-8 | Command outputs, structured diff review, unit measurements, and tested commit SHA | Pass | 2026-08-26/27: `cargo fmt --all --check` exit 0; `cargo clippy -p koduck-ai --all-targets --all-features -- -D warnings` exit 0; `cargo test -p koduck-ai --all-targets --all-features` exit 0 with all 23 test binaries `ok` (457 tests, 0 failed); `npm test --prefix tools/governance-validator` 145/145; `npm run validate --prefix tools/governance-validator` printed `Governance validation passed.`; structured diff inspection found every changed maintained source file carrying the ADR-0005 marker at its first legal comment position, zero provider/model/tag-selected coalescing branch (the only `<think>` occurrences are the neutral PLB-9 fixture content), zero new dependency, migration, or runtime variable, and no retained-limit drift outside PLB-1 through PLB-8. Implementation commit: `1494c0f` (source evidence pinned; later revisions are point-in-time supplements only). |

Allowed final check statuses are `Pass`, `Fail`, or `N/A — <specific reason>`.
`Fail` blocks completion. `N/A` is valid only when the check's stated trigger or
precondition demonstrably does not apply.

## Completion Checklist [Required]

| ID | Item | Completion Criterion | Expected Evidence | Status | Actual Evidence |
| --- | --- | --- | --- | --- | --- |
| A-1 | ADR approved | An eligible non-author approver, approval time, and exact `Approval Evidence: Approve` are recorded; any optional Approval Context Revision is informational, non-binding, and exactly represents the approved document | ADR metadata | Complete | @linhai self-identified in the task conversation and responded `Approve` for this exact record on 2026-08-26; Approver, Approval Time (2026-08-26T14:21:40Z), and `Approval Evidence: Approve` are recorded in Metadata; no Approval Context Revision is recorded because the approved content has no immutable revision yet |
| A-2 | Complete task delivered | T-1 and T-2 have actual implementation evidence, every applicable acceptance check is `Pass`, and together they satisfy the complete task outcome | Implementation Plan and Acceptance Checks rows | Complete | 2026-08-26/27: T-1 and T-2 carry per-path implementation evidence; AC-1 through AC-8 are all `Pass` with command results recorded in this record; the reproduced high-fragment response class now completes (AC-1) under the 512-Item budget with distinct resource diagnostics (AC-5). |
| A-3 | Reciprocal ADD link synchronized, when applicable | The selected candidate records this exact ADR path, this ADR records the exact ADD path and candidate ID, both references agree, and the candidate reaches `Complete` only with this ADR's `Complete` or `Verified` status | Exact ADD path, candidate ID, ADR path, and Git blob or commit | N/A — not product demand | This corrective compatibility task selects no ADD candidate |
| A-4 | Requirement levels satisfied | Every required section is complete, every conditional trigger is assessed and completed or marked `N/A — <reason>`, and optional sections are complete or removed | Structured document review | Complete | Structured review on 2026-08-26 found every required section present, every conditional trigger assessed, retained optional content complete, and zero unresolved template placeholders |
| A-5 | Acceptance checks are decidable | Every check names one subtask, preconditions or input, deterministic method, exact expected result, and evidence; no unqualified subjective criterion remains | Structured acceptance-check review | Complete | Structured review on 2026-08-26 found eight checks; each names T-1 or T-2, exact inputs, an executable method, observable numeric or state results, and expected evidence |
| A-6 | Engineering exceptions governed, when applicable | Every exceeded or waived engineering rule has one complete exception row, an accountable owner, a lifecycle, and verification evidence before approval; otherwise the conditional subsection records `N/A — <reason>` | Engineering Exceptions subsection and affected-file evidence | N/A — no exception proposed | No engineering exception is proposed; implementation must remeasure changed units |
| A-7 | Contract and baseline risks covered, when applicable | PLB-1 through PLB-9 map to explicit checks, and all five baseline risk rows are complete before approval and reach Pass before review-ready or completion | Contract-To-Check Traceability, Risk Coverage Matrix, acceptance checks, and stable evidence | Complete | 2026-08-26: every PLB row maps to at least one AC; all five Risk Coverage Matrix rows are `Pass` with per-row evidence; the cancellation row is exercised at the production HTTP disconnect boundary in AC-7. |
| A-8 | Governance validation passed | The independent validator reports no required-section, template-field, lifecycle-status, index, reciprocal-link, or Mermaid contract error for this record and repository | `npm run validate --prefix tools/governance-validator` output | Complete | On 2026-08-26, `npm test --prefix tools/governance-validator` passed all 145 tests and `npm run validate --prefix tools/governance-validator` exited 0 with `Governance validation passed.`; after acceptance, both commands were re-run on the accepted content and again passed 145/145 with `Governance validation passed.` |

## Supporting Notes [Optional]

- The 2026-08-26 local reproduction is diagnostic context only. Repository
  acceptance evidence must use deterministic fixtures and must not retain a
  live credential or third-party response body.
- `run.sh` and its credential are local operator inputs, are not affected paths,
  and must not be committed or copied into evidence.
- The 500-ms latency bound is deliberately below the existing two-second append
  deadline and far below the 30-second provider idle deadline; AC-2 measures
  timer eligibility separately from database completion latency.
- The exact SSE queue capacity is derived as 514 slots: one `turn.started`, up
  to 512 counted Item/terminal publications, and one possible in-band error.
- **Decomposition review (2026-08-27)**: measured at the implemented
  revision — production files over the 400-line review threshold are
  `runner.rs` 656 (down from a 796-line baseline; the coalescing additions
  and the extracted `runner/tool_call.rs` seam, 179 lines, keep it below the
  800-line exception limit), `ports.rs` 731, `tool_projection.rs` 665, and
  `runtime/mod.rs` 699. Each remains one cohesive consumer-owned contract
  surface whose split would separate a wire contract from its single owner;
  none crossed the 800-line exception limit, so no engineering exception is
  required. Test files over the 600-line review threshold (`cand_1_contract`
  1,192, `cand_2_runner_projection_guards` 1,419, `runtime_wiring` 986,
  `cand_1_durability` 1,057, `cand_1_liveness` 875) each group one
  boundary-family's exhaustive scenarios and stay below the 1,800-line
  exception limit. `runner::handle_event` (95 physical lines) is the one
  exhaustive per-event dispatch table over `ProviderEvent`; splitting it would
  separate each event's boundary-flush rule from its terminal arbitration.
  Cyclomatic complexity: `N/A — no configured complexity tool` (clippy's
  `too_many_lines` at 100 lines and `-D warnings` gate executable-unit span).

## Archival [Conditionally Required — Decision Status is `Rejected`, or Decision Status is `Deprecated` or `Superseded` and Implementation Status is final]

Archive this record in the same change that retires it when either Decision
Status is `Rejected` with Implementation Status `Not Applicable`, or Decision
Status is `Deprecated` or `Superseded` with Implementation Status `Verified`,
`Complete`, or `Not Applicable`. Before that trigger, retain this section as
inactive future-lifecycle guidance; its checklist does not affect acceptance or
implementation completion. When triggered:

- [ ] Move this file to
      `archive/ADR-0005-provider-delta-coalescing-and-512-item-turn-budget.md`
      under this project ADR root.
- [ ] Update every code marker that cites this file's pre-archive path to the new
      archive path, or remove the marker if the governed code was deleted.
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
| 2026-08-27 | Implemented T-1 and T-2 and recorded all evidence: coalescer module, runner integration, 512-Item budget, resource-limit diagnostics, 514-slot SSE queue, contract-copy updates, rewritten budget/grouping fixtures, and Pass on AC-1 through AC-8 and all five risk rows; Implementation Status set to `Complete`. The Affected paths enumeration gained the two in-scope focused-regression sources (`recovery_budget.rs`, `interruption_ownership.rs`), `cand_1_kernel.rs`, and the two new decomposition modules (`delta_coalescer.rs`, `runner/tool_call.rs`) as evidence-only enumeration under the already-approved "focused regressions" scope; no approved decision content changed. | @zcode |
| 2026-08-26 | Recorded @linhai's `Approve` response (Decision Status `Accepted`, Approval Time 2026-08-26T14:21:40Z) and completed checklist A-1; reworded PLB-3's opening "Pending content flushes" to "Buffered content flushes" because the accepted-stage validator treats a value-position `Pending` token as an unresolved placeholder — no flush trigger, ordering, or other approved decision content changed. | @zcode |
| 2026-08-26 | Tightened AC-1 so its sub-16-KiB, sub-500-ms completion fixture must produce exactly one durable coalesced agent delta rather than merely fewer deltas than raw provider fragments. | @codex |
| 2026-08-26 | Applied structured-review corrections: made Item 512 terminal-only, recorded RQ-1 as the durable source of the exact 512 requirement, selected synchronous `422 resource-limit-exceeded` with explicit alternatives analysis, and moved the static no-provider-branch assertion exclusively to AC-8 diff inspection. | @codex |
| 2026-08-26 | Proposed one application-orchestration slice that coalesces raw provider deltas into bounded durable Items, raises the shared post-acceptance Turn budget from 64 to 512, preserves the 1-MiB and retained limits, and distinguishes resource exhaustion from storage unavailability. | @codex |
