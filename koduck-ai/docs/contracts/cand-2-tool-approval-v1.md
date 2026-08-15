<!-- ADR: docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md -->

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
