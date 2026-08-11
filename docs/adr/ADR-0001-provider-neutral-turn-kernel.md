# ADR-0001: Provider-Neutral Tool-Free Turn Kernel

## Metadata [Required]

- **Decision Status**: Accepted
- **Implementation Status**: Not Started
- **Date**: 2026-08-11
- **Author**: @codex
- **Decision Owner**: @linhai
- **Required Approver**: @linhai
- **Record Scope**: Project
- **Approver [Conditionally Required — Decision Status is or has been `Accepted`]**: @linhai
- **Approval Time [Conditionally Required — Decision Status is or has been `Accepted`]**: 2026-08-11T10:40:42+08:00
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
- **Related [Optional]**: [Chinese translation](translations/zh-CN/ADR-0001-provider-neutral-turn-kernel.md); [Koduck Trello card 4WI4sszw](https://trello.com/c/4WI4sszw/2-%E8%B0%83%E7%A0%94-adr-%E6%98%8E%E7%A1%AE-ai-%E6%9C%8D%E5%8A%A1%E9%87%8D%E6%9E%84%E8%BE%B9%E7%95%8C%E4%B8%8E-codex-%E5%AF%B9%E9%BD%90%E7%9B%AE%E6%A0%87)
- **Architecture Source [Conditionally Required — product demand]**: `docs/architecture/ADD-0001-ai-service-codex-alignment.md` — CAND-1
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

Koduck has no service implementation in this repository yet. Its predecessor
implemented REST/SSE presentation, authentication, provider selection,
orchestration, persistence, tool use, and background work inside one Rust
service, but that infrastructure has been removed. The predecessor is therefore
functional research evidence only, not an operating, wire-contract, storage,
deployment, or rollback baseline.

`docs/architecture/ADD-0001-ai-service-codex-alignment.md` defines the target
boundary and selects CAND-1 as the first executable slice. This ADR must decide
how to implement one authenticated, tool-free turn through the new owned v1
`POST /api/v1/ai/chat` and `POST /api/v1/ai/chat/stream` surface
while introducing owned Thread, Turn, and Item types, one provider-neutral
orchestration owner, append-before-publish history, and fenced foreground
liveness. It defines an AI-owned durable Thread/Turn/Item baseline and has no
legacy compatibility, shared-history, predecessor artifact, route-back, or
runtime fallback requirement.

## Scope [Required]

In scope:

- A new Rust `koduck-ai` service crate and root Cargo workspace membership.
- Owned Thread, Turn, Item, terminal-outcome, trust-context, lease-generation,
  provider-event, and domain-error types for one foreground, tool-free turn.
- One orchestration state machine and consumer-owned provider and history ports.
- An OpenAI-compatible provider adapter exercised without live network access
  through a deterministic protocol test server.
- Presentation adapters implementing the new authenticated synchronous chat
  and SSE v1 contracts for the plain-text, no-attachment, no-tool slice.
- An AI-owned PostgreSQL history adapter supporting initial acceptance,
  append-before-publish, ordered replay, conditional terminal append, lease
  acquire/renew/fence, and orphan reconciliation.
- Contract, state-machine, durability-fault, crash, lease, and no-fallback tests
  required by the acceptance checks in this record.

Out of scope:

- Privileged tools, MCP invocation, approval, sandboxing, extensions, agent
  profiles, skills, plugins, background tasks, forks, and checkpoints.
- Semantic Memory and background Multitask integration, forks, checkpoints,
  and the expanded idempotency model reserved for CAND-3.
- More than one production provider protocol, provider fallback, or public
  typed protocol introduction beyond the owned REST/SSE v1 contract.
- Attachments, image input, memory ranking, ask/clarification flows, task APIs,
  proactive multi-agent execution, Web/native UI, deployment, and traffic
  cutover.

## Tensions, Constraints, And Open Questions [Required]

### Identified Tensions [Conditionally Required — competing goals or trade-offs exist]

| ID | Tension | Impact | Decision |
| --- | --- | --- | --- |
| TN-1 | Durable-before-visible ordering adds latency and couples streaming availability to the history path. | Publishing before append can produce client-visible items that canonical replay cannot reproduce; waiting without a bound can stall a turn indefinitely. | Require append-before-publish with a 2-second deadline per append and bounded unpublished buffering; surface a typed durability outage rather than publishing an uncommitted item. |
| TN-2 | A multi-crate architecture exposes boundaries strongly, while the first slice has only one concrete consumer and service. | Premature crates increase build and versioning overhead; one undifferentiated module would recreate predecessor coupling. | Start with one service crate organized into domain, application, and adapter modules with enforced inward dependencies; extract crates only when a later accepted decision has a second consumer or independent lifecycle. |
| TN-3 | Fast owner-loss detection improves recovery time, while short leases increase false fencing during pauses or partitions. | A false fence cancels useful work; a long lease leaves an orphan visible as active. | Renew every 5 seconds, use a 20-second lease, allow 2 seconds of clock-skew margin before reconciliation, and never transfer the same Turn to a new owner. |

### Constraints [Required]

- The authoritative architecture source is
  `docs/architecture/ADD-0001-ai-service-codex-alignment.md` CAND-1; this ADR
  may detail that slice but may not widen it.
- The architecture source returned to `Current` after the greenfield revision
  was reapproved by `@linhai` at `2026-08-11T10:37:34+08:00`; the reciprocal
  CAND-1 content agrees. This satisfies the architecture-source precondition but
  does not approve this independently governed ADR.
- The new identity/trust-context contract is authoritative. The presentation
  adapter receives a validated immutable trust context from the configured
  gateway/Auth boundary; the core does not parse
  or validate bearer tokens.
- Provider wire types, Axum request/response types, persistence records, and
  service-client types cannot enter the domain or application modules.
- A terminal Turn never becomes active again. Resume creates a new Turn on the
  same Thread from durable ordered history.
- Each externally visible Item or terminal outcome must be durably appended
  before its REST response or SSE event is published.
- The initial Turn, input Item, and lease generation must all be durable before
  the presentation boundary acknowledges acceptance. A failed initial write
  returns an error and exposes no accepted Turn.
- The unpublished buffer is capped at 64 Items and 1 MiB of serialized Item
  payload per Turn, whichever limit is reached first. Each append has a
  2-second deadline. Reaching a limit or deadline stops provider consumption,
  publishes no uncommitted Item, and returns `durability-unavailable` over the
  live REST/SSE response.
- Foreground owners renew a lease every 5 seconds. A lease expires 20 seconds
  after its last persisted renewal; reconciliation is eligible only after an
  additional 2-second clock-skew margin. Only the current generation may
  append.
- Concurrent reconcilers use one conditional key composed of Thread ID, Turn
  ID, and lease generation. Exactly one reconciler may append the orphan
  terminal outcome `cancelled`; stale owners and losing reconcilers receive a
  typed fenced result.
- Source and tests must comply with
  `docs/development/software-engineering-standard.md` and
  `docs/development/rust-standard.md`. No engineering exception is authorized
  by this draft.
- Implementation, build, deployment, or other operational work must not begin
  until this ADR is `Accepted`. Any build or deployment additionally requires
  its own accepted operational authorization under repository policy.

### Open Questions [Conditionally Required — material questions exist or were resolved during drafting]

| ID | Question | Owner | Due | Status | Resolution and Evidence |
| --- | --- | --- | --- | --- | --- |
| Q-1 | Which contract defines CAND-1 request, response, SSE event, header, status-code, and interruption behavior? | @linhai | 2026-08-12 | Resolved | The new implementation is authoritative. The Detailed Design in this ADR defines REST/SSE v1, and `koduck-ai/docs/contracts/cand-1-rest-sse-v1.md` will be its implementation copy with golden fixtures. Predecessor commit `c414ddccdbc45a99fcd3d606ca0fe1f75730b7fe` may inform scenario coverage but creates no parity requirement. Resolution follows repository-owner direction in the active Codex task on 2026-08-11. |
| Q-2 | What rollback or fallback target applies when the predecessor infrastructure has been removed? | @linhai | 2026-08-12 | Resolved | None for CAND-1: this is a greenfield, pre-deployment implementation decision. A failing candidate is not promoted and is reverted or quarantined. The first deployment requires an accepted OCR; after a verified new artifact exists, later OCRs may select only a verified new artifact. No predecessor artifact, APISIX old route, route-back, or runtime fallback is required. Resolution follows repository-owner direction in the active Codex task on 2026-08-11. |
| Q-3 | What owns initial accept, ordered append/replay, conditional terminal append, and fenced lease generation? | @linhai | 2026-08-12 | Resolved | The consumer-owned `TurnHistory` port and AI-owned PostgreSQL adapter are canonical for Thread/Turn/Item and foreground leases. Memory is reserved for later semantic-memory integration and Multitask for later background-work integration; neither is a CAND-1 history dependency. Resolution follows repository-owner direction in the active Codex task on 2026-08-11. |

## Decision Drivers [Required]

1. **Deterministic lifecycle ownership**: One application state machine must
   own valid transitions and distinguish completion, failure, authenticated
   interruption, dependency cancellation, durability outage, and owner loss.
2. **Replay fidelity**: A client must never observe a domain Item that the
   canonical history adapter cannot replay in the same order.
3. **Owned contract**: The bounded REST/SSE v1 slice must conform exactly to
   the new request, response, header, event, status, and interruption behavior
   defined by this ADR.
4. **Replaceable boundaries**: Provider, presentation, and history details must
   remain adapters around consumer-owned ports.
5. **Greenfield isolation**: The implementation must not depend on removed
   predecessor artifacts, routes, shared history, or runtime fallback.
6. **Failure containment**: Process crashes, storage outages, lease expiry, and
   concurrent reconciliation must have exact terminal and fencing outcomes.

## Options Considered [Required]

### Option A: One modular Rust service crate with owned ports and adapters

Create a new `koduck-ai` crate whose domain and application modules own the
lifecycle and port contracts. Axum, OpenAI-compatible provider, and minimal
history implementations remain adapters. Test module dependency direction and
observable contracts without creating separate crates for every boundary.

Pros:

- Establishes the required ownership and test seams with the smallest initial
  workspace and dependency surface.
- Allows later extraction by a separately justified consumer or lifecycle
  boundary without leaking framework or wire types into the core.

Cons:

- Rust crate boundaries do not mechanically prevent every internal import, so
  architecture tests and review must enforce allowed module dependencies.
- A later multi-service or reusable-library split may move modules.

### Option B: Split domain, core, protocol, provider, and history into separate crates now

Create a workspace of narrowly scoped crates before the first executable turn.

Pros:

- Cargo dependencies mechanically expose forbidden cross-boundary imports.
- Independently reusable packages are possible from the first revision.

Cons:

- No second consumer or independent release lifecycle exists yet.
- More manifests, public APIs, and integration wiring would be committed before
  their required variation is demonstrated.

### Option C: Recreate the predecessor chat flow first and extract boundaries later

Recreate the old handler/provider/persistence structure in the new repository
and defer domain ownership changes.

Pros:

- Reuses familiar functional scenarios and may produce an early endpoint.
- Reduces initial product-flow discovery work.

Cons:

- Retains the coupling and distributed state-transition ownership that CAND-1
  exists to remove.
- Makes append-before-publish and generation fencing cross-cutting retrofits
  instead of core invariants.

## Decision [Required]

**Selected option**: Option A — one modular Rust service crate with owned ports
and adapters.

**Rationale**: CAND-1 needs independent transport, provider, history, and
orchestration failure boundaries, but it has one service and one initial
consumer. A module-oriented service crate establishes inward dependency
direction and consumer-owned traits without prematurely making internal types
public across packages. It also enables deterministic in-process adapters for
contract and fault testing. Options B and C respectively add public/package
surface before demonstrated variation or preserve the coupling this task must
remove.

### Consequences [Required]

Positive:

- Thread, Turn, Item, and terminal-state invariants have one owner independent
  of REST/SSE, provider, and persistence representations.
- Presentation contract, provider protocol, and history behavior can be tested
  and replaced independently.
- Append-before-publish and lease-generation fencing become application
  invariants rather than handler conventions.
- The first implementation stays narrow enough to preserve CAND-2 through
  CAND-5 as separate decisions.

Negative:

- The CAND-1 history adapter is intentionally limited to foreground turns;
  CAND-3 expands it for lineage, checkpoints, semantic-memory projections, and
  background-work integration without transferring canonical ownership.
- Durability latency becomes part of response latency, and a storage outage
  stops streaming after the durable prefix.
- Module dependency rules require a dedicated architecture test and review
  until a later decision justifies crate extraction.

Mitigations:

- Keep all external representations in adapters and define the canonical
  history surface only through the CAND-1 consumer-owned port.
- Enforce the 2-second append deadline and unpublished-buffer caps and capture
  latency/failure evidence in deterministic tests.
- Add a dependency-direction test that rejects Axum, provider-wire, and
  persistence imports from domain/application source paths.

### Detailed Design [Required]

The service uses this inward dependency direction:

```text
REST/SSE adapter ─┐
Provider adapter ─┼─> application turn runner ─> domain lifecycle and values
PostgreSQL adapter ─┘           │
                                └─> consumer-owned provider/history ports
```

The domain lifecycle is:

```text
started ──> completed
   │      ├> failed
   │      ├> interrupted
   │      └> cancelled
   └──────> recovery-pending ──> failed
                          └────> cancelled
```

`completed`, `failed`, `interrupted`, and `cancelled` are terminal. Provider
errors end in `failed`; an authenticated client stop ends in `interrupted`;
platform/dependency stop or fenced owner loss ends in `cancelled`. A history
append failure after acceptance moves the live owner to `recovery-pending`,
stops provider consumption, and exposes only the durable prefix plus the
transport diagnostic `durability-unavailable`. When history returns, one
conditional terminal append closes it as `failed`, except that an already
expired and fenced owner is closed as `cancelled` by reconciliation.

The application layer consumes two ports:

- `ModelProvider`: accepts owned model input and emits ordered owned deltas,
  usage, completion, or a typed provider error. The first adapter translates
  an OpenAI-compatible chat-completions stream; no provider JSON type crosses
  the adapter boundary.
- `TurnHistory`: atomically accepts initial Turn/input/lease state, appends an
  Item under the expected generation, reads ordered history, renews the current
  generation, conditionally fences an expired generation, and conditionally
  appends one terminal outcome under the Thread/Turn/generation key.

The first `TurnHistory` adapter uses PostgreSQL as shared durable state. It owns
four logical relations: `threads` for tenant-scoped Thread identity,
`turns` for lifecycle and the next sequence, `turn_items` for append-only Items
and terminal outcomes keyed by `(turn_id, sequence)`, and `turn_leases` for the
current generation and persisted expiry. Initial Thread/Turn/input/lease state
commits in one transaction. Each append locks or conditionally updates the Turn
under its expected lease generation, allocates exactly one next sequence, and
commits the Item before publication. Terminal uniqueness is enforced per Turn;
fence/reconcile compares both generation and expiry and can append only one
terminal. Tenant and Thread ownership are present in every read/write predicate.
No process-local store, Memory client, Multitask client, predecessor service, or
alternate adapter may be selected as a CAND-1 runtime fallback.

For both REST and SSE, the presentation adapter obtains an immutable validated
trust context and maps the owned v1 request into a new Turn command.

The authoritative v1 wire contract is:

- `POST /api/v1/ai/chat` accepts JSON with an optional UUID `thread_id` and a
  non-empty UTF-8 string `input` of at most 65,536 bytes, using
  `Content-Type: application/json`; unknown fields are rejected.
  Success is `200` with JSON containing exactly `thread_id`, `turn_id`,
  `status`, `items`, and `usage`; `status` is `completed`, and every item has
  exactly `item_id`, a positive integer `sequence`,
  `type: agent_message_delta`, and non-empty string `content`. `usage` contains
  exactly non-negative integer `input_tokens`, `output_tokens`, and
  `total_tokens`, with `total_tokens = input_tokens + output_tokens`.
- `POST /api/v1/ai/chat/stream` accepts the same JSON and returns `200` with
  `Content-Type: text/event-stream`. Event names are `turn.started`,
  `item.created`, and exactly one of `turn.completed`, `turn.failed`,
  `turn.interrupted`, or `turn.cancelled`. Each event's JSON includes matching
  `thread_id`, `turn_id`, and a strictly increasing positive `sequence`;
  `turn.started` data contains exactly `thread_id`, `turn_id`, `sequence`, and
  `status: started`; `item.created` additionally contains exactly `item_id`,
  `type: agent_message_delta`, and non-empty `content`; terminal data contains
  exactly `thread_id`, `turn_id`, `sequence`, and its matching `status`, plus
  the exact `usage` object above only for `turn.completed`.
- `POST /api/v1/ai/turns/{turn_id}/interrupt` returns `202` with exactly
  `turn_id` and `status: interrupt-requested`, with no other fields, when the
  caller's validated trust context owns an active Turn. The request has no
  body. The stream later emits one durable
  `turn.interrupted` terminal. Unknown or non-owned Turns return `404` without
  revealing which condition applied; already-terminal Turns return `409` with
  error code `turn-already-terminal`.
- Missing or invalid validated identity returns `401`, includes
  `WWW-Authenticate: Bearer`, and uses `application/problem+json` with error
  code `invalid-identity`. Invalid JSON/input returns `400`; initial or
  mid-turn durability failure returns `503` with code
  `durability-unavailable`. Every error body contains exactly `type:
  about:blank`, `title`, numeric `status`, stable `code`, and UUID
  `correlation_id`; `title` is the code converted from kebab case to words with
  its first letter capitalized. No error response exposes an accepted Turn
  when the initial durable transaction failed.

Synchronous chat buffers only already-durable Items before returning its
response. SSE publishes each event only after the corresponding Item or
terminal append succeeds. Resume loads prior durable history and creates a
different Turn ID on the same Thread; it does not mutate the prior terminal
Turn. The contract copy and golden fixtures are implementation evidence, not a
second source of authority.

## Implementation Plan [Required]

**Complete task outcome**: A provider-neutral Thread/Turn/Item kernel executes
one authenticated plain-text, tool-free Turn through both owned REST/SSE v1
routes using the OpenAI-compatible provider path, and deterministic evidence
shows ordered durable replay plus the exact completion, provider-failure,
authenticated-interruption, durability-outage, crash, lease-expiry,
stale-owner, concurrent-reconciler, and no-legacy-fallback outcomes defined here.

Allowed subtask statuses: `Not Started`, `In Progress`, `Blocked`, `Complete`,
or `N/A — <specific reason>`.

| ID | Objective or deliverable | Included scope | Status | Actual implementation evidence |
| --- | --- | --- | --- | --- |
| T-1 | Create the owned domain lifecycle, application turn runner, consumer-owned ports, and one OpenAI-compatible provider adapter. | Root Cargo workspace; `koduck-ai` domain, application, provider adapter, typed errors, unit tests, and dependency-direction test. | Not Started | Pending |
| T-2 | Implement the owned authenticated REST/SSE v1 contract and freeze its golden fixtures. | Plain-text no-tool `POST /api/v1/ai/chat`, `POST /api/v1/ai/chat/stream`, and interrupt route; trust-context handoff; request/response/header/status/SSE fixture hashes; contract tests. | Not Started | Pending |
| T-3 | Implement the AI-owned PostgreSQL history and fenced-liveness adapter and prove failure/recovery/no-fallback behavior. | Initial durable acceptance, append/replay, migrations, deadlines and buffer caps, lease acquire/renew/fence, orphan reconciliation, crash/fault tests, and proof of no legacy runtime dependency. | Not Started | Pending |

**Affected paths**: `Cargo.toml`; `koduck-ai/Cargo.toml`;
`koduck-ai/src/domain/**`; `koduck-ai/src/application/**`;
`koduck-ai/src/adapters/http/**`; `koduck-ai/src/adapters/provider/**`;
`koduck-ai/src/adapters/history/**`; `koduck-ai/src/main.rs`;
`koduck-ai/migrations/**`; `koduck-ai/tests/**`;
`koduck-ai/docs/contracts/cand-1-rest-sse-v1.md`;
`docs/adr/ADR-0001-provider-neutral-turn-kernel.md`;
`docs/adr/translations/zh-CN/ADR-0001-provider-neutral-turn-kernel.md`;
`docs/adr/INDEX.md`; and
`docs/architecture/ADD-0001-ai-service-codex-alignment.md`; and
`docs/architecture/translations/zh-CN/ADD-0001-ai-service-codex-alignment.md`.

**Migration and rollback strategy [Conditionally Required — this replaces or
changes existing behavior]**: N/A — this is a greenfield source decision and no
existing Koduck AI runtime, predecessor artifact, APISIX old route, shared
history, or fallback path exists. A candidate that fails any acceptance check
must not be promoted and may be reverted or quarantined with its evidence
preserved. This ADR does not authorize deployment. The first deployment requires
an accepted OCR with an immutable new artifact and recovery procedure; after a
verified new artifact exists, later rollback may target only a verified new
artifact under an accepted OCR.

### Engineering Exceptions [Conditionally Required — an engineering rule is exceeded or waived]

N/A — the proposed design does not exceed or waive a repository engineering
rule. Any exception discovered during implementation is approval-invalidating
and must be added here before the affected source change proceeds.

## Acceptance Checks [Required]

| Check ID | Subtask | Binary acceptance point | Preconditions or input | Verification method | Exact expected result | Expected evidence | Status | Actual result and evidence |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| AC-1 | T-1 | Domain and application source has no Axum, provider-wire, or persistence-adapter dependency. | T-1 source exists. | Run `cargo test -p koduck-ai --test architecture domain_and_application_dependencies_are_inward -- --exact`. | Exit code 0; the test reports zero forbidden imports or Cargo dependencies from `src/domain/**` and `src/application/**`. | Command output and tested commit. | Not Started | Pending |
| AC-2 | T-1 | The same authenticated input and deterministic provider stream produces one ordered Turn lifecycle independent of adapter representation. | In-process provider emits deltas `A`, `B`, usage, and completion; in-memory history port accepts all appends. | Run `cargo test -p koduck-ai --test cand_1_kernel tool_free_turn_completes_with_ordered_items -- --exact`. | Exit code 0; one Turn reaches `completed`; replay order is input, `A`, `B`, usage, terminal; every published sequence number equals its durable append sequence. | Command output and serialized replay fixture. | Not Started | Pending |
| AC-3 | T-1 | A provider terminal error produces `failed` and never `completed`. | In-process provider emits `A` then error code `UPSTREAM_RESET`. | Run `cargo test -p koduck-ai --test cand_1_kernel provider_error_is_failed_terminal -- --exact`. | Exit code 0; durable replay contains input, `A`, and exactly one `failed` terminal carrying `UPSTREAM_RESET`; it contains zero `completed` terminals. | Command output and replay fixture. | Not Started | Pending |
| AC-4 | T-2 | The synchronous route conforms to the owned REST v1 contract. | Valid trust context; new Thread request with input `hello`; deterministic provider response `A`; golden fixture generated from this ADR's contract. | Run `cargo test -p koduck-ai --test cand_1_contract sync_chat_v1_contract -- --exact`. | Exit code 0; status is `200`; `Content-Type` is `application/json`; the canonicalized body contains exactly `thread_id`, `turn_id`, `status`, `items`, and `usage`; `status` is `completed`; IDs are UUIDs; item sequences are positive and strictly increasing; after replacing only UUIDs and usage counters with fixture tokens, body and required headers equal the owned v1 golden fixture. | Command output, fixture hash, and comparison report. | Not Started | Pending |
| AC-5 | T-2 | The SSE route conforms to the owned event contract and publishes no event before its durable append. | Valid trust context; provider emits two deltas and completion; owned v1 fixture hash is recorded. | Run `cargo test -p koduck-ai --test cand_1_contract sse_v1_contract_and_append_before_publish -- --exact`. | Exit code 0; status is `200`; content type is `text/event-stream`; events are one `turn.started`, two ordered `item.created`, and exactly one `turn.completed`; all IDs match and sequences strictly increase; every publish observation has a lower-sequence successful append observation for the same Item or terminal identity. | Command output, fixture hash, and append/publish trace. | Not Started | Pending |
| AC-6 | T-2 | Resume creates a new Turn on the same Thread and does not mutate the prior terminal Turn. | One completed Turn exists with its immutable replay hash. | Run `cargo test -p koduck-ai --test cand_1_contract resume_creates_new_turn -- --exact`. | Exit code 0; resumed Turn ID differs, Thread ID matches, prior replay hash is unchanged, and the new provider input contains the ordered durable history exactly once. | Command output and before/after replay hashes. | Not Started | Pending |
| AC-7 | T-2 | The interrupt contract is exact and an authenticated client stop remains distinct from platform/dependency cancellation. | One owned active SSE Turn, one unknown UUID, one non-owned Turn, one already-terminal Turn, and one second Turn with an injected dependency stop. | Run `cargo test -p koduck-ai --test cand_1_contract interrupt_and_cancel_are_distinct -- --exact`. | Exit code 0; the owned active case returns `202` with only its `turn_id` and `status: interrupt-requested`, then exactly one durable `interrupted`; unknown and non-owned cases both return indistinguishable `404` problem bodies after correlation-ID normalization; the terminal case returns `409` with code `turn-already-terminal`; the dependency-stopped Turn has exactly one `cancelled`; no tested Turn has another terminal. | Command output, normalized response hashes, and replay fixtures. | Not Started | Pending |
| AC-8 | T-3 | Initial history failure exposes no accepted Turn, while a later append outage exposes only the durable prefix plus `durability-unavailable`. | Fault adapter fails initial transaction in case A and fails the next append after one durable delta in case B. | Run `cargo test -p koduck-ai --test cand_1_durability initial_and_mid_turn_outages_fail_closed -- --exact`. | Exit code 0; case A has zero accepted Turn records and zero provider calls; case B publishes only the durable delta, publishes no failed append payload, stops provider consumption, and emits `durability-unavailable`. | Command output, adapter trace, and replay fixtures. | Not Started | Pending |
| AC-9 | T-3 | Append deadline and unpublished-buffer limits are exact and fail closed. | Virtual clock; cases for a 2.001-second append, 65 Items, and payload size 1,048,577 bytes. | Run `cargo test -p koduck-ai --test cand_1_durability append_deadline_and_buffer_caps -- --exact`. | Exit code 0; each case stops provider consumption, publishes zero over-limit Items, emits `durability-unavailable`, and retains a replay equal to the pre-case durable prefix. | Command output and per-case trace. | Not Started | Pending |
| AC-10 | T-3 | Process-crash reconciliation fences the expired generation and appends one orphan `cancelled` terminal. | Virtual clock; last renewal at t=0; 5-second heartbeat, 20-second lease, 2-second skew margin; owner process terminates immediately after one durable delta. | Run `cargo test -p koduck-ai --test cand_1_liveness process_crash_fences_and_cancels_once -- --exact`. | Exit code 0; reconciliation before t=22 seconds is rejected, reconciliation at t=22 seconds fences the generation, exactly one `cancelled` terminal is durable after the delta, and every old-generation append after fencing returns `FENCED`. | Command output and lease/append trace. | Not Started | Pending |
| AC-11 | T-3 | Concurrent reconcilers and delayed store recovery cannot duplicate or overwrite the orphan terminal. | 32 reconcilers race on one expired Thread/Turn/generation while the store is unavailable and then recovers. | Run `cargo test -p koduck-ai --test cand_1_liveness concurrent_reconcilers_are_idempotent -- --exact`. | Exit code 0; after recovery exactly one conditional write succeeds, durable history has exactly one `cancelled` terminal, 31 reconcilers receive `ALREADY_TERMINAL` or `FENCED`, and a late `completed` append is rejected. | Command output, race summary, and replay hash. | Not Started | Pending |
| AC-12 | T-3 | CAND-1 has no runtime dependency or fallback to predecessor infrastructure, Memory, or Multitask. | T-1 through T-3 source, manifests, configuration schema, and migrations exist. | Run `cargo test -p koduck-ai --test architecture cand_1_has_no_legacy_or_external_history_fallback -- --exact`. | Exit code 0; dependency inspection reports zero predecessor repository/artifact/route identifiers, zero Memory or Multitask clients in the CAND-1 execution graph, and exactly one canonical `TurnHistory` implementation configured: the AI-owned PostgreSQL adapter. | Command output and dependency/configuration report. | Not Started | Pending |
| AC-13 | T-2 | A request without a validated trust context reaches neither the application Turn runner nor the provider/history ports. | Request omits or carries invalid identity; owned v1 error contract is loaded. | Run `cargo test -p koduck-ai --test cand_1_contract invalid_identity_stops_at_presentation_boundary -- --exact`. | Exit code 0; status is `401`; `WWW-Authenticate` is `Bearer`; content type is `application/problem+json`; body contains exactly `type: about:blank`, `title: Invalid identity`, numeric `status: 401`, `code: invalid-identity`, and a UUID `correlation_id`; provider call count, initial history-write count, and accepted Turn count are all zero. | Command output, response fixture hash, and adapter call counters. | Not Started | Pending |

Allowed final check statuses are `Pass`, `Fail`, or `N/A — <specific reason>`.
`Fail` blocks completion. `N/A` is valid only when the check's stated trigger or
precondition demonstrably does not apply.

## Completion Checklist [Required]

| ID | Item | Completion Criterion | Expected Evidence | Status | Actual Evidence |
| --- | --- | --- | --- | --- | --- |
| A-1 | ADR approved | An eligible non-author approver, approval time, and exact `Approval Evidence: Approve` are recorded; any optional Approval Context Revision is informational, non-binding, and exactly represents the approved document | ADR metadata | Complete | `@linhai` self-declared the approval identity, identified ADR-0001, and responded with exact `Approve`; metadata records `2026-08-11T10:40:42+08:00`. No Approval Context Revision is recorded because no immutable revision yet represents the approved uncommitted content. |
| A-2 | Complete task delivered | Every declared subtask has actual implementation evidence, every applicable acceptance check is `Pass` with actual result and evidence, and together they satisfy the complete task outcome | Implementation Plan and Acceptance Checks rows | Not Started | Pending |
| A-3 | Reciprocal ADD link synchronized, when applicable | The selected candidate records this exact ADR path, this ADR records the exact ADD path and candidate ID, both references agree, and the candidate reaches `Complete` only with this ADR's `Complete` or `Verified` status | Exact ADD path, candidate ID, ADR path, and Git blob or commit | Not Started | Pending — reciprocal links agree, but the checklist item remains incomplete until implementation evidence is recorded and CAND-1 can truthfully reach `Complete`. |
| A-4 | Requirement levels satisfied | Every required section is complete, every conditional trigger is assessed and completed or marked `N/A — <reason>`, and optional sections are complete or removed | Structured document review | Complete | Structured review confirms Q-1 through Q-3 are resolved, the governing ADD is `Current`, every required section is populated, and every conditional trigger is completed or records a specific `N/A`. |
| A-5 | Acceptance checks are decidable | Every check names one subtask, preconditions or input, deterministic method, exact expected result, and evidence; no unqualified subjective criterion remains | Structured acceptance-check review | Complete | Structured review confirms all 13 checks name one subtask, deterministic inputs/methods, exact observable outcomes, and evidence; no unqualified subjective success criterion remains. |
| A-6 | Engineering exceptions governed, when applicable | Every exceeded or waived engineering rule has one complete exception row, an accountable owner, a lifecycle, and verification evidence before approval; otherwise the conditional subsection records `N/A — <reason>` | Engineering Exceptions subsection and affected-file evidence | N/A — no exception proposed | Engineering Exceptions records `N/A` and requires an approval-invalidating update if implementation discovers an exception. |

## Supporting Notes [Optional]

- The predecessor baseline at commit
  `c414ddccdbc45a99fcd3d606ca0fe1f75730b7fe` demonstrates chat/stream scenarios
  and an `LlmProvider` abstraction plus OpenAI-compatible adapters. It is
  functional research evidence only: no route, field, artifact, datastore, or
  fallback behavior from it is authoritative for CAND-1.
- The 2-second append deadline, 64-Item/1-MiB unpublished caps, 5-second
  heartbeat, 20-second lease, and 2-second skew margin are approval-sensitive
  decision values. Changing any of them after acceptance requires the
  approval-invalidating workflow.

## Archival [Conditionally Required — Decision Status is `Rejected`, or Decision Status is `Deprecated` or `Superseded` and Implementation Status is final]

Archive this record in the same change that retires it when either Decision
Status is `Rejected` with Implementation Status `Not Applicable`, or Decision
Status is `Deprecated` or `Superseded` with Implementation Status `Verified`,
`Complete`, or `Not Applicable`. Before that trigger, retain this section as
inactive future-lifecycle guidance; its checklist does not affect acceptance or
implementation completion. When triggered:

- [ ] Move this file to
      `archive/ADR-0001-provider-neutral-turn-kernel.md` under this project ADR
      root (`docs/adr/archive/`).
- [ ] Keep the non-authoritative Chinese translation at
      `docs/adr/translations/zh-CN/ADR-0001-provider-neutral-turn-kernel.md`,
      update its authoritative-English backlink to this record's archived path,
      and update this record's translation link for its new directory depth.
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
| 2026-08-11 | Created the Proposed project Full ADR by selecting CAND-1, including detailed boundaries, exact timing and buffering decisions, three subtasks, deterministic acceptance checks, and unresolved acceptance preconditions. | @codex |
| 2026-08-11 | Added a non-authoritative Chinese translation and linked it from the authoritative English ADR without creating a second decision identity or index row. | @codex |
| 2026-08-11 | Reset Completion Checklist items A-3 through A-5 from `In Progress` to `Not Started` and aligned their evidence with unresolved approval prerequisites. | @codex |
| 2026-08-11 | Resolved Q-1 through Q-3 from repository-owner direction in the active Codex task: made the new REST/SSE v1 and AI-owned PostgreSQL history contracts authoritative, removed legacy parity/shared-history/APISIX route-back and runtime fallback requirements, and rewrote subtasks and acceptance checks for the greenfield boundary. | @codex |
| 2026-08-11 | Synchronized the architecture-source gate after `@linhai` reapproved ADD-0001 as `Current` at `2026-08-11T10:37:34+08:00`; ADR Decision Status remains independently `Proposed`. | @codex |
| 2026-08-11 | Accepted after the human approver self-declared `@linhai`, identified ADR-0001, and responded with exact `Approve`; recorded Approval Time `2026-08-11T10:40:42+08:00`. Implementation Status remains `Not Started`, and no Approval Context Revision is recorded because the approved content is not yet represented by an immutable commit. | @linhai |
