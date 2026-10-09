# CAND-18 Chat Submission Identity Contract (v1)

<!-- ADR: docs/adr/ADR-0018-chat-submission-identity-and-atomic-acceptance.md -->

Status: implementation copy. The authoritative decision and the complete
normative text of every clause below live in
`docs/adr/ADR-0018-chat-submission-identity-and-atomic-acceptance.md`
(Submission Contract, SI-01 through SI-10). This document is evidence of the
implemented public shape, never a second decision authority. It extends the
authoritative CAND-1 v1 contract (`cand-1-rest-sse-v1.md`) additively; every
unlisted request and response shape, error body, and lifecycle behavior of
that contract remains authoritative and unchanged.

## Request extension (SI-01)

Both `POST /api/v1/ai/chat` and `POST /api/v1/ai/chat/stream` accept the
existing `input` and optional `thread_id` members plus one new optional
member:

```json
{ "input": "text", "thread_id": null, "submission_id": "0f0e0d0c-0b0a-4987-8654-321098765432" }
```

- `submission_id` MUST be absent, or a non-nil UUID encoded as exactly 36
  ASCII characters in hyphenated form. Hexadecimal case is ignored when
  parsing.
- An explicit `null`, an empty string, a non-string value, the nil UUID, or
  any other UUID presentation (simple, braced, URN) returns
  `400 invalid-request` with no history operation. Parsing distinguishes an
  absent member from a present null.
- Duplicate members, unknown members, malformed JSON, empty input, or input
  above 65,536 UTF-8 bytes return `400 invalid-request` with no history
  operation (SI-01b). Missing or invalid authenticated identity returns the
  existing `401 invalid-identity` with the Bearer challenge before any
  lookup, provider call, or write (SI-01c).
- Omission keeps the existing fresh-submission behavior: no binding is
  written and no deduplication guarantee applies (SI-01a, SI-03a, SI-10).

## Retry semantics (SI-02, SI-05)

The operation key is `(tenant_id, subject_id, submission_id)` from the
validated trust context and the parsed UUID. Equality compares the exact
UTF-8 bytes of the original sequence-1 `user_message` and the originally
requested Thread selector. Clients MUST preserve the originally requested
selector on retries: when the original request omitted or null-selected a
Thread, keep that selector even after `turn.started` reports an allocated
`thread_id`; adding that UUID to the retry is semantic drift.

For an exact accepted-key retry on either route, before any SSE header or
event:

```json
{
  "submission_id": "0f0e0d0c-0b0a-4987-8654-321098765432",
  "thread_id": "11111111-2222-4333-8444-555555555555",
  "turn_id": "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee",
  "status": "accepted"
}
```

- HTTP `202`, media type `application/json`, exactly these four members,
  UUIDs in canonical lowercase hyphenated text.
- `accepted` describes the immutable acceptance outcome, not current Turn
  lifecycle or inference success. The same receipt is returned for started,
  recovery-pending, completed, failed, interrupted, and cancelled Turns
  while their canonical data remains present. The retry returns no Items or
  usage, never waits for the original producer, never subscribes to its
  stream, and never sends SSE.
- A first `Created` invocation keeps the existing synchronous or SSE
  lifecycle response and cancellation behavior unchanged.
- Changed semantic input or selector under an owned existing key returns
  `409 submission-identity-conflict`; unavailability returns
  `503 durability-unavailable`; both use the existing exact problem body and
  expose no accepted identities (SI-02d, SI-05).

## Durability and authority (SI-03, SI-04, SI-06, SI-07, SI-08)

The binding (key, original selector, accepted Thread/Turn, and a
server-private creator-operation UUID) commits in the same PostgreSQL
transaction as the initial Turn, sequence-1 input, and generation-1 lease,
guarded by a unique constraint on the scoped key and a unique accepted-Turn
constraint (`migrations/0010_cand_18_chat_submissions.sql`). Only the
invocation whose private creator-operation identity committed the binding
receives execution authority; every other invocation — including a later
exact retry after completion, expiry, interruption, or process restart —
receives only the receipt above. Commit acknowledgement loss is settled by
one write attempt plus at most one read-only reconciliation inside the same
invocation, under the separate two-`int4` submission advisory-lock namespace
derived per SI-03b, with a ten-second monotonic acceptance deadline and the
cancellation-first remaining-4-second write gate. Bindings live for the
canonical Turn's lifetime with no TTL and no independent deletion.
