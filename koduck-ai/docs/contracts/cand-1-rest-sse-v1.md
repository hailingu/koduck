<!-- ADR: docs/adr/ADR-0001-provider-neutral-turn-kernel.md -->

# CAND-1 REST/SSE v1 Implementation Contract

This file is the implementation copy of the authoritative wire contract in
`docs/adr/ADR-0001-provider-neutral-turn-kernel.md`. It is test evidence, not a
second source of authority. When the two differ, the Accepted ADR governs.

## Trust Boundary

The presentation adapter receives an immutable validated `TrustContext` from
the configured gateway/Auth boundary. A missing or invalid identity returns
`401`, `WWW-Authenticate: Bearer`, and `application/problem+json`; the request
does not reach the turn runner, provider, or history ports.

## Synchronous Chat

`POST /api/v1/ai/chat` requires `Content-Type: application/json`. Its request
object contains non-empty UTF-8 string `input` of at most 65,536 bytes and an
optional UUID `thread_id`; unknown and duplicate fields are rejected.

Success is `200 application/json`. The body contains exactly `thread_id`,
`turn_id`, `status`, `items`, and `usage`. `status` is `completed`. Each item
contains exactly `item_id`, positive strictly increasing `sequence`,
`type: agent_message_delta`, and non-empty `content`. Usage contains exactly
non-negative `input_tokens`, `output_tokens`, and `total_tokens`; the total is
their sum. Only durably appended items are returned.

## Streaming Chat

`POST /api/v1/ai/chat/stream` accepts the same request and returns
`200 text/event-stream`. It emits one `turn.started`, zero or more ordered
`item.created`, and exactly one terminal event named `turn.completed`,
`turn.failed`, `turn.interrupted`, or `turn.cancelled`. Every event carries
matching Thread and Turn IDs and a positive strictly increasing sequence.

`turn.started` data contains exactly `thread_id`, `turn_id`, `sequence`, and
`status: started`. `item.created` additionally contains exactly `item_id`,
`type: agent_message_delta`, and non-empty `content`. Terminal data contains
exactly `thread_id`, `turn_id`, `sequence`, and matching `status`, plus the
exact usage object only for `turn.completed`. Each visible item or terminal is
published only after its durable append succeeds.

## Interrupt

`POST /api/v1/ai/turns/{turn_id}/interrupt` has no body. An owned active Turn
returns `202 application/json` with exactly `turn_id` and
`status: interrupt-requested`; its stream later emits one durable
`turn.interrupted`. Unknown and non-owned Turns return indistinguishable `404`
problem responses. A terminal Turn returns `409` with code
`turn-already-terminal`. Dependency/platform cancellation remains the distinct
terminal `turn.cancelled`.

## Problems

Invalid JSON or input returns `400`. Initial or mid-turn durability failure
returns `503` with code `durability-unavailable`. Every problem body contains
exactly `type: about:blank`, kebab-code-derived `title`, numeric `status`, stable
`code`, and UUID `correlation_id`. Failed initial acceptance exposes no Turn.

## Golden Fixtures

Fixture hashes include the final newline:

| Fixture | SHA-256 |
| --- | --- |
| `koduck-ai/tests/fixtures/sync-chat-v1.json` | `96d28d8f1670f3e089f66d60b3ecf2fbb906a5ea7d652304e66e6235dfe29629` |
| `koduck-ai/tests/fixtures/sse-v1.txt` | `503d90fb076c2a66869e873801082e47003f3fa0ceb13218c8e38f1ccf1f6ba8` |
| `koduck-ai/tests/fixtures/invalid-identity-v1.json` | `3dbd2d782374da9d70e7dab6c5d49037257e780769b37e853deee21d179c9729` |
