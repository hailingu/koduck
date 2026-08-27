<!-- ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md -->
<!-- ADR: docs/adr/ADR-0005-provider-delta-coalescing-and-512-item-turn-budget.md -->

# CAND-2 Tool Approval v1 Implementation Contract

This file is the implementation copy of the authoritative contract in
`docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md`. It is
test evidence, not a second source of authority. When the two differ, the
Accepted ADR governs. Sections land incrementally with the T-2 transport
deliverables; only implemented behavior is documented here.

## Trust Boundary And Gateway-Validated Approval Scopes

The presentation adapter receives an immutable validated `TrustContext` from
the configured gateway/Auth boundary (C-7), as in CAND-1. For ADR-0003 the
gateway-validated context channel additionally carries the principal's
approval scopes in the `x-koduck-approval-scopes` header, following the
repository-owner direction of 2026-08-14 to continue the CAND-1 gateway-trust
model: the configured gateway validates signed claims, injects the header as
part of the validated context, and is responsible for stripping any
client-forwarded value at the trust boundary.

`koduck-ai` only seals what that boundary already validated:

- An absent header yields a trusted context with no approval scopes.
- A present header must be a comma-separated list of at most 16 scope tokens,
  each 1–128 bytes of ASCII alphanumerics plus `.`, `_`, `:`, and `-`.
- Whitespace is not normalized and is not part of the grammar: surrounding or
  embedded whitespace, empty tokens, oversized tokens, forbidden characters,
  and over-count values all invalidate the whole identity (`401`, like any
  invalid identity), because the gateway issues canonical comma-separated
  values only and never emits malformed context.

The gateway strip-and-reissue rule that protects this header is normative and
is recorded with the identity handoff in
[`../runtime-configuration.md`](../runtime-configuration.md): the gateway must
remove any caller-supplied `X-Koduck-Approval-Scopes` value and set the header
only from the scopes its validated signed claims actually grant. The runtime
performs no independent signed-claim validation of this header.

The sealed scopes attach only to the gateway-validated identity; request
bodies, Tool/MCP content, and model output can never add or widen them. Only a
same-tenant principal whose sealed scopes contain `ai.tool.approve` may resolve
a requested approval (TC-05).

## Authenticated Approval Decision Route

The Axum runtime exposes `POST /api/v1/ai/approvals/{approval_id}/decisions`
through the framework-neutral `ApprovalDecisionAdapter` over
`ApprovalDecisionRoute<SqlxApprovalRecordStore>`:

- The body is exactly one JSON object containing only
  `decision: accepted | declined | cancelled`; duplicate or unknown members,
  any other value, a non-JSON body, or a non-`application/json` content type is
  `400 invalid-request` before any decision.
- Missing identity is `401 invalid-identity` with
  `WWW-Authenticate: Bearer`; a non-POST method is `405 method-not-allowed`.
- The request must carry one `X-Koduck-Thread-Id` header naming the approval's
  canonical Thread. It is client-supplied routing context, not authority: the
  conditional durable lookup also requires the gateway-validated tenant, the
  authenticated requester subject, and the approval identity. An absent,
  malformed, or mismatched Thread value — like an unscoped, cross-tenant,
  cross-Thread, non-owning, or unknown approval identity — returns one
  indistinguishable `404 not-found` and mutates no record.
- The first winning decision returns `200` with the canonical projection
  `{"approval_id", "status", "decision", "version"}`; an identical replay
  returns the same projection; any conflicting resolution is
  `409 approval-already-resolved`; store unavailability is
  `503 durability-unavailable`.

## Native Tool And MCP Call Translation

Both adapter origins address one configured capability through the same owned
translation (`ConfiguredCapability`, `translate_native_tool_call`,
`translate_mcp_tool_call` in `koduck-ai/src/adapters/tool.rs`):

- The effect, target, descriptor ID, and version come only from the trusted
  C-5 descriptor snapshot; untrusted wire content can never relabel them
  (ADR-0003 TC-01/TC-03).
- The serialized parameters and arguments use the duplicate-aware fail-closed
  JSON translation with the 65,536-byte input bound applied before parsing.
- An MCP server-declared name that does not address exactly the configured
  capability is rejected before any owned action exists; the translated value
  is byte-identical to the native Tool translation of the same call, so the
  isolated executor observes one envelope format regardless of origin
  (ADR-0003 TC-11).

## D-3 Projections

C-5 appends ordered durable views of canonical D-6/D-7 state through the
consumer-owned `ToolProjectionSink` port
(`koduck-ai/src/application/tool_projection.rs`):

- `ToolProjection::ApprovalStatus` carries the canonical D-6 identity and its
  exact bound D-7 identity, plus status, decision, and record version;
  `ToolProjection::ToolCall` and
  `ToolProjection::ToolResult` carry the canonical D-7 identity, lifecycle
  phase, stable failure code, executor effect state, and transition version
  (`prepared` = 1, `running` = 2, terminal = 3).
- `append` performs the durable append and `publish` makes the projection
  visible; publication happens only after the append succeeded, and a failed
  append suppresses publication without changing canonical state
  (ADR-0003 TC-06).
- Projections are write-only views: no API accepts a projection as approval,
  descriptor, profile, or dispatch authority, and replaying or forging one
  causes zero additional D-6 records, dispatches, or terminals.
- The driver emits the approval-status and terminal-result projections; the
  coordinator emits the running projection immediately after the canonical
  dispatch claim wins and before any executor call.

## Authenticated C-5 Interruption

The application boundary `ToolInterruptionRoute`
(`koduck-ai/src/application/tool_interruption.rs`) drives the guarded C-5
cancellation path for one authenticated Turn interruption:

- The tenant comes only from the gateway-validated `TrustContext`; the Thread
  comes only from validated routing context. An absent Thread context, an
  unknown Turn, and a cross-tenant principal are one indistinguishable
  `NoLiveAttempt` with zero service calls and zero mutations, so the route
  cannot be used to probe live-work existence (ADR-0003 TC-05/TC-10).
- Canonical subject ownership is validated through the consumer-owned
  `TurnOwnershipValidator` port before the authority catalog is touched: a
  same-tenant non-owner or an unknown identity is the same indistinguishable
  `NoLiveAttempt` and neither cancels another subject's work nor leaves an
  interruption tombstone behind, while a canonical-ownership outage fails
  closed as `ReconciliationRequired/DurabilityUnavailable` with zero
  mutations.
- For authenticated owners, a prepared D-7 and its requested D-6 close as
  `cancelled/not_started` with zero dispatch; a running D-7 receives exactly
  one bounded executor cancellation whose acknowledgement determines the
  terminal — acknowledged state commits `cancelled` with the exact
  executor-observed effect state, and a missing acknowledgement commits
  `timed_out/unknown` (ADR-0003 TC-10).
- The route introduces no new authority-mutation call site: it forwards to the
  shared `ExecutionInterrupter` over the runtime's process-owned authority
  catalog, with the cancellation and approval ports supplied by runtime
  assembly. The production composition lands with the T-3 durable D-7
  committer that supplies those ports.

## Provider Tool-Call Translation And Runner Servicing

The OpenAI-compatible adapter assembles streamed `tool_calls` fragments into
complete owned calls: fragments merge by `index`, a repeated name for one
index, a missing index, a non-object function fragment, or a
`finish_reason: "tool_calls"` frame with no assembled call fails closed with
`INVALID_TOOL_CALL_FRAME`, and the assembled calls emit in index order as
`ProviderEvent::ToolCall { name, arguments }` before `[DONE]` (ADR-0003
TC-02/TC-11). Assembly is bounded incrementally before any allocation grows:
one call's cumulative arguments never exceed the canonical 65,536-byte
serialized action input (`TOOL_CALL_ARGUMENTS_TOO_LARGE`), and a 33rd
assembled call fails closed (`TOO_MANY_TOOL_CALLS`): the 32-call assembly
bound is exact and retained unchanged under the raised 512-item per-Turn
budget (ADR-0005 PLB-8). A `[DONE]` frame arriving
while assembled fragments remain unflushed (no `finish_reason: "tool_calls"`)
fails closed as `INVALID_TOOL_CALL_FRAME` instead of dropping the requested
action.

A Tool-call round's stream end is not a Turn completion: after servicing, the
runner starts a continuation request whose `ModelInput.tool_rounds` carries
every bounded committed result — the C-5 driver's return proves the
current-generation durable commit before the result reaches the model
(TC-11) — and the adapter serializes each round as its own assistant
`tool_calls` message followed by its `tool` result messages, so alternating
assistant-call/result groups preserve the causal order of a call raised on an
earlier round's result; synthesized call identities are unique across the
whole request. Usage counters from the initial request and every continuation
are summed with checked overflow into the completed Turn terminal (a counter
overflow fails closed as `PROVIDER_USAGE_OVERFLOW`). A `Completed` event on a
stream that still owes a continuation is a provider protocol violation and
fails the Turn closed as `PROVIDER_PREMATURE_COMPLETION`.

The runner services each event through the consumer-owned `ToolCallExecutor`
port and owns the durable append-before-publish ordering. The port returns
only the bounded committed `ModelToolResult`; every D-3 view
(`approval_status`, `tool_call`, `tool_result`) streams through the
runner-supplied `TurnProjectionSink`, whose `append` performs the durable
append and whose `publish` is the visibility step (ADR-0003 TC-06). The sink
is seeded with the runner's cumulative per-Turn provider counters and
synchronizes them back when the call returns, so one Turn's projections share
the single 512-item/1-MiB provider buffer allowance with every coalesced
provider item (ADR-0001, ADR-0005 PLB-5). Before persisting, the sink validates each projection's canonical
tuple — the status/decision/version shape, the exact `prepared` = 1 /
`running` = 2 / terminal = 3 transition versions, and the canonical Tool
value validators for the descriptor, version, and target fields — and tracks
the call's lifecycle stage bound to the open canonical D-6/D-7 identity, so a
resolution or terminal view that references a different record is rejected.
Each projection's complete item sequence is preflighted atomically against
the cumulative budget before any part of it is appended, and a projection
that opens a lifecycle reserves worst-case capacity for its guaranteed
remainder before that first append: a running view is never appended without
capacity for its terminal view, and a requested approval never without
capacity for its resolution, dispatch, and terminal views, so no orphan
running or approval view can be left durable; reservations are released as
the guaranteed projections land. `publish` forwards each successfully
appended item to the live observer immediately — the SSE `item.created` event
for a requested approval or running transition is visible throughout the
approval wait or executor call, not after the port returns. A rejected or
failed append fails the sink closed, so no later append resumes the
incomplete lifecycle, and the Turn terminalizes as a durability boundary
violation that takes precedence over any executor error; any other turn-level
port failure owns the turn terminal with its stable code. A `TurnRunner`
without an assembled boundary records every call as a typed
`tool_execution_unavailable` result without executing it (TC-13).

The production runtime assembles `BoundaryToolCallExecutor`
(`koduck-ai/src/runtime/tool_executor.rs`) over the process's sole C-5
authority root and the empty descriptor snapshot: an unresolved name denies as
`descriptor_missing`, a resolved capability without a bound profile denies as
`outside_permission_profile` — both with zero D-6/D-7 and zero dispatch — and
the empty inventory makes every production call take the denial path
(TC-02/TC-13). The approval decision provider fails closed (`cancelled`); an
interactive decision bridge requires its own accepted capability record. Both the dispatch and interruption paths validate the bound foreground lease
generation through the injected durable `SqlxTurnLeaseValidator` (reading the
canonical `turn_leases` rows under the two-second arbitration window), and
every D-7 terminal commits through the injected durable
`SqlxExecutionAttemptStore` conditional transitions — the terminal write, its
correlated audit append, and the Turn-barrier arbitration are one atomic
durable sequence, with the process-local authority catalog only arbitrating
preparation and dispatch ordering (ADR-0003 TC-07/TC-12/TC-14).
