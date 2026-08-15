<!-- ADR: docs/adr/ADR-0001-provider-neutral-turn-kernel.md -->
<!-- ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md -->

# Koduck AI Runtime Configuration

This document is the configuration schema for the CAND-1 executable. The
Accepted ADR remains authoritative when this implementation copy differs.

## Required Environment

| Variable | Format | Purpose | Sensitive |
| --- | --- | --- | --- |
| `KODUCK_AI_BIND_ADDR` | Socket address such as `127.0.0.1:8080` | Axum listener address | No |
| `KODUCK_AI_DATABASE_URL` | PostgreSQL connection URL | Canonical Thread, Turn, Item, and lease storage | Yes |
| `KODUCK_AI_OPENAI_BASE_URL` | HTTPS base URL ending before `/chat/completions` | Explicit OpenAI-compatible provider | No |
| `KODUCK_AI_OPENAI_MODEL` | Non-empty provider model identifier | Tool-free chat-completions model | No |
| `KODUCK_AI_OPENAI_API_KEY` | Provider bearer credential | Provider authentication | Yes |

Every variable is required and blank values are rejected. Runtime debug output
redacts both the database URL and provider credential. Operators must supply
secrets through the deployment's approved secret mechanism; they must not be
committed, logged, or placed in Trello or review evidence.

## Validated Identity Handoff

The Axum boundary accepts `X-Koduck-Tenant-Id` and `X-Koduck-Subject-Id` only as
identity already validated by the configured gateway/Auth boundary. The
gateway must remove caller-supplied values and set both headers from its
validated identity. A deployment must prevent direct untrusted access to the
AI listener; that topology and its verification require an Accepted OCR.

The same handoff governs approval authority (ADR-0003 TC-05): the gateway must
also remove any caller-supplied `X-Koduck-Approval-Scopes` value and set that
header only from the scopes its validated signed claims actually grant. The
runtime seals whatever the header carries into `TrustContext` as approval
authority, so a forwarded caller-supplied value would be an approval-scope
injection; the runtime performs no independent signed-claim validation of this
header. A deployment that cannot enforce the strip-and-reissue rule at the
gateway must not expose the approval decision route.

Missing, blank, or non-UTF-8 identity values produce the owned `401
invalid-identity` response before the turn runner, provider, or history ports
are called. A missing `X-Koduck-Approval-Scopes` header yields a trusted
context with no approval scopes; a present but malformed value (empty tokens,
whitespace or other forbidden characters, oversized tokens, or more than 16
tokens) invalidates the whole identity the same way.

## Approval Decision Route And Thread Routing Context

The runtime exposes the authenticated ADR-0003 decision route
`POST /api/v1/ai/approvals/{approval_id}/decisions` (ADR-0003 TC-05). The
request body is exactly one JSON object containing only
`decision: accepted | declined | cancelled`. The route requires the
gateway-validated tenant, subject, and `ai.tool.approve` scope headers above,
plus one `X-Koduck-Thread-Id` request header carrying the canonical Thread of
the approval. The Thread header is client-supplied routing context, not
authority: the durable lookup additionally requires the gateway-validated
tenant, the authenticated requester subject, and the approval identity, so an
absent, malformed, or wrong Thread value only fails closed as an
indistinguishable `404` and can never widen what a principal may resolve. The
gateway strip-and-reissue rule is not required for this header because it
carries no identity or scope.

The route is served through the production canonical D-6 assembly:
`ApprovalDecisionRoute<SqlxApprovalRecordStore>` over the shared PostgreSQL
pool, with each conditional transition limited by the 2-second attempt
deadline and mapped to `durability-unavailable` (`503`) on expiration.

## Startup

The executable connects to PostgreSQL and applies the idempotent CAND-1
history schema plus the idempotent ADR-0003 CAND-2 approval schemas
(`0002_cand_2_policy_execution.sql` and
`0003_cand_2_requester_ownership.sql`), with each startup operation limited by
the 2-second database-attempt deadline, constructs exactly one
`PostgresTurnHistory<SqlxPostgresExecutor>` and one
`ApprovalDecisionRoute<SqlxApprovalRecordStore>` over the shared pool,
constructs the configured OpenAI-compatible transport, binds the listener, and
exposes only the four owned v1 routes. Startup fails explicitly if
configuration, PostgreSQL, provider-client construction, listener binding, or
HTTP serving fails. No process-local, Memory, Multitask, predecessor, or
alternate history fallback is configured.

## Operational Bounds

- Provider connection establishment is limited to 5 seconds, response headers
  to 30 seconds, inactivity between response body chunks to 30 seconds, and
  total response processing to 120 seconds. A deadline produces a provider
  failure and closes the accepted Turn through the normal terminal path.
- Every synchronous PostgreSQL operation uses the approved 2-second attempt
  deadline and maps expiration to `durability-unavailable`.
- Lease-renewal and failed-append recovery tasks share admission for at most
  256 background workers per production `PostgresTurnHistory` instance.
  On an append outage, the Turn stops its renewal worker and atomically moves
  that worker's permit into bounded recovery without decrementing shared
  admission between owners. Recovery therefore retains reserved capacity even
  when all 256 slots were occupied. The original streaming observer remains
  attached until recovery closes the Turn or defers to expiry reconciliation;
  each renewal/recovery database attempt remains capped at 2 seconds and the
  recovery owner is capped by the 22-second reconciliation window. Other
  saturation rejects new work with `durability-unavailable`. If a separately
  scheduled recovery worker cannot be created, the scheduling owner runs the
  same bounded recovery while retaining its permit.
- The interrupt endpoint accepts only an owned `started` Turn whose lease is
  unfenced and has not passed its 2-second skew window. Before returning `202`,
  one transaction locks that ownership, appends the unique durable
  `interrupted` Item, and commits the terminal status. Provider or recovery
  writers racing afterward replay that terminal. A `recovery-pending` or
  expired Turn rejects interruption because its original SSE observer can no
  longer be guaranteed to deliver `turn.interrupted`.
