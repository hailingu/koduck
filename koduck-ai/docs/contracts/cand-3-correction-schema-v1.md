<!-- ADR: koduck-ai/docs/adr/ADR-0003-correction-item-schema-and-raw-replay.md -->

# CAND-3 Correction Schema v1 Implementation Contract

This file is the implementation copy of the authoritative correction
representation contract in
`koduck-ai/docs/adr/ADR-0003-correction-item-schema-and-raw-replay.md`. It is
test evidence, not a second source of authority. When the two differ, the
Accepted ADR governs.

## Domain Representation

A correction is one distinct append-only Item kind,
`koduck_ai::domain::ItemPayload::Correction`, carrying one non-blank
replacement `content` string and one `corrects_item_id`
(`koduck_ai::domain::ItemId`) predecessor identity. Construction through
`koduck_ai::domain::item_correction::ItemCorrection::new` rejects blank
content with `DomainValueError::Empty`; no admission, length, predecessor
kind, or caller-identity policy is defined here — CAND-11 owns it. Every
pre-existing Item kind keeps its exact representation.

## Durable Storage

- Discriminator: `turn_items.item_type = 'correction'` (migration
  `0009_cand_3_correction_items.sql` extends the immutable 0001–0008
  sequence; the migration is forward-only and idempotent and preserves every
  existing row and constraint).
- Payload JSON: exactly `{"content": <string>}`. The predecessor identity is
  not duplicated in the payload; it lives only in the relationship column.
- Relationship column: `turn_items.corrects_item_id UUID NULL`.
- Structural constraints:
  - `corrects_item_id` is not null if and only if `item_type` is
    `'correction'`, and a correction row is never terminal
    (`turn_items_correction_shape`).
  - A correcting Item must not identify itself
    (`turn_items_correction_not_self`).
  - The predecessor must exist in the same tenant, Thread, and Turn
    (`turn_items_correction_scope` composite foreign key).
  - One predecessor has at most one direct correction successor
    (partial unique index `turn_items_one_direct_correction`).

## Codec

`koduck_ai::adapters::history::postgres::DurableItemCodec` is the public
durable Item codec contract. `encode` returns the durable column tuple for
one `ItemPayload`; `decode(item_type, payload_text, corrects_item_id)`
returns the owned payload. Decoding fails closed with
`application::HistoryError::Unavailable` for an unknown discriminator,
malformed JSON, a correction payload that is not exactly the one `content`
string member (any missing, non-string, blank, extra, or duplicated member
— the payload is deserialized once by a strict document type that rejects
duplicate and unknown members while constructing the owned value, with no
intermediate full-document allocation), a missing relationship identity on
a correction row, or a relationship identity present on any non-correction
row. No stored row is guessed, dropped, or rewritten; a non-canonical
payload shape is never silently canonicalized into replay history.

## Raw Replay

`replay` returns every original and correction Item exactly once in strictly
increasing Turn-local sequence order and never updates, deletes, substitutes,
hides, or resequences an Item. Before returning, the executor validates the
decoded Turn through
`koduck_ai::domain::item_correction::validate_raw_replay`, which fails closed
with a stable `RawReplayStructureError` for a non-increasing sequence, a
duplicate Item identity, a self-correction, a target absent from the
replayed Turn, or a second direct successor of one predecessor. Sequence
adjacency is not required.

## Non-Integration

This contract adds no correction admission operation, no effective
projection, no provider serialization, no REST/SSE event, and no Memory
delivery. Provider input construction and the HTTP wire treat a correction
Item as inert: it produces no provider message and no `item.created`
document.
