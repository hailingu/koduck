# Lightweight ADR-0002: Typed HTTP Wire Serialization

## Metadata [Required]

- **Decision Status**: Accepted
- **Implementation Status**: Complete
- **Date**: 2026-08-25
- **Author**: @codex
- **Decision Owner**: @linhai
- **Required Approver**: @linhai
- **Record Scope**: Service internal — koduck-ai
- **Approver [Conditionally Required — Decision Status is or has been `Accepted`]**: @linhai
- **Approval Time [Conditionally Required — Decision Status is or has been `Accepted`]**: 2026-08-25T05:35:25Z
- **Approval Evidence [Conditionally Required — Decision Status is or has been `Accepted`]**: Approve
- **Approval Context Revision [Optional — informational and non-binding]**: `edb8392` — the first immutable revision containing the approved `Proposed` document content; informational only, not approval evidence.
- **Rejector [Conditionally Required — Decision Status is `Rejected`]**: N/A — Decision Status is `Proposed`
- **Rejection Time [Conditionally Required — Decision Status is `Rejected`]**: N/A — Decision Status is `Proposed`
- **Rejection Evidence [Conditionally Required — Decision Status is `Rejected`]**: N/A — Decision Status is `Proposed`
- **Retired By [Conditionally Required — Decision Status is `Deprecated` or `Superseded`]**: N/A — Decision Status is `Proposed`
- **Retirement Time [Conditionally Required — Decision Status is `Deprecated` or `Superseded`]**: N/A — Decision Status is `Proposed`
- **Retirement Evidence [Conditionally Required — Decision Status is `Deprecated` or `Superseded`]**: N/A — Decision Status is `Proposed`
- **Retirement Reason [Conditionally Required — Decision Status is `Deprecated` or `Superseded`]**: N/A — Decision Status is `Proposed`
- **Blocked From [Conditionally Required — Implementation Status is `Blocked`]**: N/A — Implementation Status is `Not Started`
- **Blocker And Evidence [Conditionally Required — Implementation Status is `Blocked`]**: N/A — Implementation Status is `Not Started`
- **Blocker Owner [Conditionally Required — Implementation Status is `Blocked`]**: N/A — Implementation Status is `Not Started`
- **Blocker Exit Or Recheck Criterion [Conditionally Required — Implementation Status is `Blocked`]**: N/A — Implementation Status is `Not Started`
- **Related [Optional]**: `docs/adr/ADR-0001-provider-neutral-turn-kernel.md`; `docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md`; `koduck-ai/docs/adr/ADR-0001-strict-json-duplicate-member-validation.md`
- **Architecture Source [Conditionally Required — product demand]**: N/A — corrective internal maintainability work discovered through source review, not derived from product demand
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

## Context [Required]

`koduck-ai/src/adapters/http/wire.rs` owns the service's outbound JSON bodies
and SSE data documents. At source revision
`fb6329d5f0231522a2b5011656f15f04cabab7fa`, it constructs those JSON documents
through `format!`, explicit fragments, comma joining, and helper functions that
render strings, UUIDs, optional values, and usage objects. Dynamic strings use
`serde_json` escaping and existing regressions prove the complete JSON control
range, so this ADR does not correct a demonstrated malformed-output defect.

The maintenance risk is duplicated wire-shape ownership. `sse_body` and
`stream_event_body` independently match every published `ItemPayload` and
repeat the field order, explicit `null` representation, string mapping, and
terminal usage rules. The two paths currently share only selected helpers and
one approval-decision parity test. A later projection field or lifecycle state
can therefore be changed in one path without the other, while isolated string
fragments make exact omission and escaping rules harder to review.

The accepted CAND-1 golden fixtures and CAND-2 D-3 projection contracts already
define the observable behavior. The intended change is a localized,
fully-reversible implementation refactor: encode those existing shapes as
private typed `serde::Serialize` DTOs, serialize them through one private
operation, and give buffered and live SSE publication one shared item-data
mapping without changing any public byte, field, event, error, or ordering
contract.

## Decision [Required]

Replace outbound JSON string assembly inside `adapters::http::wire` with
private typed wire DTOs derived with the existing `serde::Serialize` support.
The DTO fields are declared in the existing emitted order, use borrowed strings
and numeric values where possible, and convert UUIDs to their existing textual
form without enabling another Cargo feature. Do not use a generic
`serde_json::Value` or map for ordered outbound objects.

Create one private item-data serializer that exhaustively translates every
published `ItemPayload` into its exact `item.created` JSON document. Both
`sse_body` and `stream_event_body` call this operation, so the payload-to-wire
mapping has one owner. Likewise, use one terminal-data operation for buffered
and live terminal events. Keep SSE framing and event-name selection explicit:
`event: <name>\ndata: <JSON>\n\n` is transport framing, not JSON serialization.

Model optionality according to the current contract rather than applying one
blanket serde rule. Fields that currently exist with `null` remain present and
serialize `None` as JSON `null`; terminal `usage` remains omitted entirely for
failed, interrupted, and cancelled outcomes and present only for completed
outcomes. Separate typed shapes may be used where that distinction is clearer
than conditional attributes.

Keep existing infallible internal wire-function signatures. The private serde
operation may use a documented `expect` only while every DTO contains
infallibly serializable primitives, strings, sequences, and options and no
custom fallible serializer or non-string map key. Introducing a fallible field
invalidates that local invariant and requires explicit error propagation plus
reclassification before implementation.

The following clauses are normative for this implementation:

- **TW-01 — Synchronous byte compatibility**: Successful synchronous chat JSON
  must remain byte-identical to the owned CAND-1 fixture after only the existing
  UUID and usage-counter normalization.
- **TW-02 — SSE byte and order compatibility**: Buffered and live SSE must
  retain exact event framing, event names, strictly increasing sequence order,
  current JSON field order, terminal selection, and completed-only usage.
- **TW-03 — D-3 item compatibility**: `agent_message_delta`,
  `approval_status`, `tool_call`, and `tool_result` item data must retain their
  exact key order, wire names, explicit-null fields, numeric fields, and escaped
  string values; buffered and live serialization of the same item identity
  must be byte-identical.
- **TW-04 — Non-item document compatibility**: Interrupt and problem bodies
  must retain their exact field order and values; problem correlation IDs
  remain UUIDs and the title remains the stable kebab-code-derived form.
- **TW-05 — JSON validity and escaping**: Every synchronous body and every SSE
  `data:` JSON document must parse successfully, preserve all Unicode and JSON
  control characters, and expose exactly the contract-declared fields.
- **TW-06 — Internal-only change**: No public symbol, accepted request,
  response field, event, error mapping, ordering rule, dependency, Cargo
  feature, contract document, migration, or golden fixture changes.

## Scope [Required]

In scope:

- Private `Serialize` DTOs and one documented primitive-only serialization
  operation inside `koduck-ai/src/adapters/http/wire.rs`.
- The governed-file marker
  `// ADR: koduck-ai/docs/adr/ADR-0002-typed-http-wire-serialization.md` at
  the first legal comment position in `koduck-ai/src/adapters/http/wire.rs`.
- One exhaustive item-data mapping shared by buffered and live SSE paths.
- One terminal-data mapping shared by buffered and live SSE paths.
- Typed synchronous, interrupt, problem, usage, and SSE data documents that
  preserve TW-01 through TW-06.
- Focused unit characterization for every item variant, explicit-null and
  omitted fields, special-character escaping, and buffered/live parity.

Out of scope:

- Any public REST/SSE contract, field, ordering, event, status, media type,
  golden fixture, error code, or accepted-input change.
- Request deserialization, strict duplicate-member input validation, provider
  parsing, persistence codecs, domain types, or application orchestration.
- Changes to `koduck-ai/src/adapters/http/mod.rs` function signatures or error
  mappings.
- New dependencies, Cargo features, public DTOs, generic JSON utilities, or a
  repository-wide serialization facade.
- Performance optimization beyond avoiding unnecessary owned copies in the
  private DTOs; no benchmark or throughput claim is authorized.

## Lightweight Eligibility Check [Required]

- [x] Change is limited to the paths in Scope and can be fully reverted without migration or retained partial behavior.
- [x] Change modifies only localized source behavior and its tests; it does not modify normative governance documentation or configuration.
- [x] No public API, schema, protocol, service boundary, auth, security, data model, migration, dependency, build, CI, or deployment behavior changes.
- [x] No new technology, framework, storage, queue, runtime, provider, or design pattern.
- [x] No material product or technical question remains unresolved.
- [x] Verification can use a deterministic focused test, static check, or inspection that produces an exact observable result and stable evidence.

## Implementation Plan [Required]

**Complete task outcome**: One independently reviewable `koduck-ai` change
replaces outbound JSON string assembly with private typed DTO serialization,
gives buffered and live SSE one item/terminal mapping, and passes TW-01 through
TW-06 without changing any public wire byte or contract.

**Primary implementation boundary**: `koduck_ai::adapters::http::wire`, the
HTTP presentation adapter's private outbound JSON and SSE representation
boundary.

Allowed subtask statuses: `Not Started`, `In Progress`, `Blocked`, `Complete`,
or `N/A — <specific reason>`.

| ID | Objective or deliverable | Included scope | Status | Actual implementation evidence |
| --- | --- | --- | --- | --- |
| T-1 | Characterize and replace non-item and synchronous JSON assembly with private typed DTOs. | `sync_body`, interrupt/problem/usage and started/terminal data, primitive-only serializer, exact-shape unit tests. | Complete | On commits `db6c1f9`/`52a1ca3`/`8fdc920`: `sync_body`, `interrupt_body`, and `problem_body` serialize `SyncTurnDocument`, `InterruptDocument`, and `ProblemDocument` through the primitive-only `wire_json` operation; the ADR governed-file marker sits at the first legal comment position of `koduck-ai/src/adapters/http/wire.rs`; exact-shape unit tests live in `adapters::http::wire::tests::typed_non_item_documents_preserve_exact_wire_shapes`. |
| T-2 | Establish one typed item-data mapping for buffered and live SSE. | Every published `ItemPayload`, including all terminal outcomes; exact null/omission/order/escaping rules; buffered/live byte-parity table. | Complete | On commits `db6c1f9`/`52a1ca3`: `item_created_event` and `terminal_turn_event` are the single owners of the item and terminal documents and are called by both `sse_body` and `stream_event_body`; `buffered_and_live_item_documents_are_byte_identical` drives every published payload — including all four terminal outcomes — through both paths and asserts byte-equal event blocks. |
| T-3 | Verify the complete localized refactor and record stable evidence. | Focused checks, CAND-1 fixtures, CAND-2 projections, routed Rust commands, governance validation, and source/diff review. | Complete | On commit `8fdc920`: `cargo fmt --all --check` OK; `cargo clippy -p koduck-ai --all-targets --all-features -- -D warnings` clean; `cargo test -p koduck-ai --all-targets --all-features` green across 21 test binaries (447 passed, 0 failed); `npm test --prefix tools/governance-validator` 145/145; `npm run validate --prefix tools/governance-validator` passed; `git diff fb6329d..8fdc920` touches only wire source/tests plus this ADR and `docs/adr/INDEX.md`. |

### Stable Implementation Touchpoints [Required]

| Path | Stable symbol or contract anchor | Key code excerpt, when needed | Purpose | Source revision |
| --- | --- | --- | --- | --- |
| `koduck-ai/src/adapters/http/wire.rs` | `sync_body`; `sse_body`; `stream_event_body` | N/A — stable functions identify the duplicated synchronous, buffered, and live wire mappings. | Owns JSON documents and both SSE publication paths that the typed DTOs must consolidate. | `fb6329d5f0231522a2b5011656f15f04cabab7fa` |
| `koduck-ai/src/adapters/http/wire.rs` | `terminal_event`; `stream_terminal_event`; `usage_json`; `json_string`; `approval_decision_wire`; `optional_uuid`; `optional_version` | N/A — stable helper symbols identify exact current omission, null, escaping, and terminal rules. | Provides the compatibility baseline replaced by typed fields and shared terminal serialization. | `fb6329d5f0231522a2b5011656f15f04cabab7fa` |
| `koduck-ai/src/adapters/http/wire.rs` | `interrupt_body`; `problem_body`; `sse_event` | N/A — stable symbols identify non-item JSON and transport framing. | Moves JSON bodies to DTOs while deliberately retaining explicit SSE framing. | `fb6329d5f0231522a2b5011656f15f04cabab7fa` |
| `koduck-ai/tests/cand_1_contract.rs` | `sync_chat_v1_contract`; `sse_v1_contract_and_append_before_publish`; `interrupt_and_cancel_are_distinct`; `invalid_identity_stops_at_presentation_boundary` | N/A — stable contract-test symbols and their owned fixtures are sufficient. | Proves exact CAND-1 success, SSE, interrupt, and problem compatibility. | `fb6329d5f0231522a2b5011656f15f04cabab7fa` |
| `koduck-ai/tests/wire_json_regressions.rs` | `response_escapes_the_complete_json_control_range` | N/A — stable test symbol identifies complete control-character coverage. | Proves valid JSON and exact decoded string preservation across sync and SSE. | `fb6329d5f0231522a2b5011656f15f04cabab7fa` |

### Engineering Exceptions [Conditionally Required — an engineering rule is exceeded or waived]

N/A — the planned localized consolidation is expected to reduce repeated
executable code in the existing 500-line wire module and does not exceed or
waive an engineering rule. Reassess before approval if the design requires
another maintained source module or crosses an exception limit.

## Contract-To-Check Traceability [Required]

| Clause ID | Authoritative contract path and heading | Exact normative requirement | Acceptance check or deterministic test IDs | Explicit coverage method |
| --- | --- | --- | --- | --- |
| TW-01 | This ADR — Decision; `koduck-ai/docs/contracts/cand-1-rest-sse-v1.md` — Synchronous Chat / Golden Fixtures | The normalized synchronous success body remains byte-identical to the owned fixture with exact fields and values. | AC-1, AC-4 | Existing fixture comparison and full routed suite exercise the production adapter. |
| TW-02 | This ADR — Decision; `koduck-ai/docs/contracts/cand-1-rest-sse-v1.md` — Streaming Chat / Golden Fixtures | SSE framing, order, event names, field order, terminal selection, and completed-only usage remain exact. | AC-1, AC-2, AC-4 | Existing fixture comparison covers CAND-1; focused parity tests cover both buffered and live functions. |
| TW-03 | This ADR — Decision; `koduck-ai/docs/contracts/cand-2-tool-approval-v1.md` — D-3 Projections; `docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md` — TC-06 | Every published item variant retains exact fields, nulls, wire names, numbers, escaping, and buffered/live byte parity. | AC-2, AC-4 | One exhaustive item table serializes every payload/state fixture through both paths and compares exact data bytes and parsed keys. |
| TW-04 | This ADR — Decision; `koduck-ai/docs/contracts/cand-1-rest-sse-v1.md` — Interrupt / Problems | Interrupt and problem JSON retain exact shape, status/code/title, and UUID correlation identity. | AC-1, AC-3, AC-4 | Existing black-box contract checks plus a focused typed non-item table assert exact documents after UUID normalization. |
| TW-05 | This ADR — Decision; `koduck-ai/docs/contracts/cand-1-rest-sse-v1.md` — Synchronous Chat / Streaming Chat | Every outbound JSON document remains valid and preserves Unicode and the complete control-character range. | AC-2, AC-3, AC-4 | Focused item/non-item tables parse every data document; existing control-range regression verifies decoded equality. |
| TW-06 | This ADR — Decision | The refactor adds no public surface, dependency, feature, contract, fixture, or behavior change. | AC-1, AC-2, AC-3, AC-4 | Focused and full checks plus stable-symbol and diff review prove the implementation-only boundary. |

## Risk Coverage Matrix [Required]

| Risk dimension | Applicability and scenario, or specific N/A reason | Owning boundary | Deterministic verification method | Exact expected result | Acceptance check IDs | Status | Actual evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Concurrency and ordering | Applicable — consolidation could change buffered/live event order, item sequence, JSON field byte order, or terminal placement. | `adapters::http::wire` SSE mapping and framing | Run AC-1, AC-2, and AC-4 fixture/parity/order checks. | Existing event and sequence order remains exact; the same item identity has byte-identical buffered/live data; fixture comparisons pass. | AC-1, AC-2, AC-4 | Pass | On `8fdc920` AC-1, AC-2, and AC-4 all exit 0: the CAND-1 fixture comparisons and strictly increasing sequence assertion in `sse_v1_contract_and_append_before_publish` pass unchanged, and every parity fixture's buffered block equals its live block byte-for-byte. |
| Timeout and deadline | N/A — DTO serialization performs no I/O, wait, retry, timer, or deadline operation and does not change caller deadlines. | `adapters::http::wire` | Inspect the private DTO/serializer symbols and run strict Clippy plus AC-4. | No timeout/deadline constant, wait, I/O, retry, or async operation is added or changed. | AC-4 | N/A — serialization performs no I/O, wait, retry, timer, or deadline operation | Symbol inspection on `8fdc920`: `wire_json` and every `*Document` DTO contain only primitive, string, sequence, and option fields with no async, timer, I/O, or retry construct; strict Clippy and AC-4 exit 0. |
| Cancellation and interruption | N/A — the refactor changes representation only; Turn cancellation/interruption arbitration and publication control stay outside `wire.rs`. | HTTP/application orchestration outside the changed representation boundary | Run AC-1 and AC-4, including the existing interrupt/cancel distinction contract. | Interrupt remains the exact 202 body; cancelled and interrupted terminal events remain distinct; no cancellation control path changes. | AC-1, AC-4 | N/A — representation-only change; arbitration stays outside `wire.rs` | On `8fdc920` AC-1 exits 0 including `interrupt_and_cancel_are_distinct`; the task diff shows zero changes outside wire source/tests, so no cancellation control path changed. |
| Resource bounds and backpressure | Applicable — DTO construction could clone unbounded strings, duplicate item buffers, or make a previously local serialization failure externally observable. | `adapters::http::wire` private DTO and serialization operation | Run AC-2 through AC-4 and inspect DTO ownership plus the primitive-only serialization invariant. | DTOs borrow dynamic payload strings where possible, introduce no retained queue or additional unbounded collection, retain existing response/event construction bounds, and all checks exit 0 without a new error surface. | AC-2, AC-3, AC-4 | Pass | On `52a1ca3`/`8fdc920` the DTO string fields borrow payload text (`&'a str` for content, descriptor, target, code, digest) and own only the small UUID text; no queue or unbounded collection was added; the single `expect` is the documented primitive-only `wire_json` invariant; AC-2 through AC-4 exit 0. |
| Framework or trust-boundary rejection | Applicable — malformed outbound JSON, incorrect problem shape, or wrong explicit-null handling would violate the HTTP presentation boundary. | `adapters::http::wire` JSON documents consumed by Axum response assembly | Parse every AC-2/AC-3 data fixture and run AC-1 black-box adapter contracts. | Every body/data document parses, exposes exactly the declared keys and null/omission state, and retains the exact content type/status/event mapping. | AC-1, AC-2, AC-3 | Pass | On `8fdc920` every AC-2/AC-3 fixture parses with exactly its declared ordered keys, explicit nulls, and omission rules, and the AC-1 black-box contract cases retain exact status, media type, fields, codes, and titles. |

Allowed final statuses are `Pass`, `Fail`, or `N/A — <specific reason>`. `Fail`
blocks review-ready, `Complete`, and `Verified`.

## Acceptance Checks [Required]

| Check ID | Subtask | Binary acceptance point | Preconditions or input | Verification method | Exact expected result | Expected evidence | Status | Actual result and evidence |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| AC-1 | T-1 | CAND-1 synchronous, SSE, interrupt, and problem output remains byte- and behavior-compatible under TW-01, TW-02, and TW-04. | Typed DTO implementation is wired; existing owned fixtures and contract cases are unchanged. | Run `cargo test -p koduck-ai --test cand_1_contract`. | Exit 0; normalized sync and SSE bodies equal their owned fixtures; append-before-publish ordering remains true; interrupt/cancel and problem cases retain exact status, media type, fields, codes, titles, and UUID correlation identity. | Command output, unchanged fixture hashes, and exact assertions. | Pass | On `8fdc920` the command exits 0 with 11 passed / 0 failed; `git diff fb6329d..8fdc920 -- koduck-ai/tests/fixtures koduck-ai/docs/contracts` is empty, so the golden fixtures and contracts are byte-unchanged; all fixture-equality, ordering, interrupt/cancel, and problem assertions pass unchanged. |
| AC-2 | T-2 | Every published item variant satisfies TW-02, TW-03, and TW-05 through one shared mapping. | A table contains agent content with control/Unicode characters; every approval decision/null state; ToolCall optional fields populated and null; ToolResult codes/effect/digest/version populated and null; completed terminal with usage; failed, interrupted, and cancelled terminals without usage; stable common IDs and sequence. | Run `cargo test -p koduck-ai --lib adapters::http::wire::tests::buffered_and_live_item_documents_are_byte_identical -- --exact`. | Exit 0; each buffered/live item data document, including every terminal outcome, is byte-identical, parses as JSON, has exactly the declared ordered keys and values, preserves escaped content on decode, represents each optional field as explicit `null` or its typed value, and includes usage only for the completed terminal. | Command output and per-fixture raw/parsed assertions. | Pass | The test was observed green against the pre-refactor `format!` implementation on `db6c1f9` and exits 0 with 1 passed on the DTO implementation at `8fdc920`; every fixture's buffered block equals its live block byte-for-byte, each document parses with its exact ordered keys, control/Unicode content round-trips, optional fields carry explicit `null`, and `usage` appears only for the completed terminal. |
| AC-3 | T-1 | Every typed non-item document satisfies TW-04 and TW-05 while control-character escaping remains complete. | Started/terminal outcomes, interrupt, problems, sync usage, Unicode, and bytes U+0000 through U+001F are represented in the focused and existing regression fixtures. | Run `cargo test -p koduck-ai --lib adapters::http::wire::tests::typed_non_item_documents_preserve_exact_wire_shapes -- --exact`; run `cargo test -p koduck-ai --test wire_json_regressions`. | Both commands exit 0; every raw body/data value has the exact declared ordered keys and omission rules, every value parses, UUID correlation IDs validate, and decoded content equals the complete input character sequence. | Command outputs and raw/parsed exact assertions. | Pass | Both commands exit 0 on `8fdc920` (1 passed and 3 passed respectively, including `response_escapes_the_complete_json_control_range`); started/terminal/interrupt/problem/synchronous documents equal their exact literals after UUID substitution, correlation IDs parse as UUIDs, and the full U+0000–U+001F range decodes equal to its input through both the live item document and the synchronous body. |
| AC-4 | T-3 | The localized refactor passes every routed check with no public, dependency, contract, fixture, or unrelated source change. | T-1 and T-2 complete; accepted contracts, manifests, lock file, fixtures, and unrelated source match the task base. | From the repository root run `cargo fmt --all --check`; `cargo clippy -p koduck-ai --all-targets --all-features -- -D warnings`; `cargo test -p koduck-ai --all-targets --all-features`; `npm test --prefix tools/governance-validator`; `npm run validate --prefix tools/governance-validator`; then inspect the stable symbols and task diff. | Every command exits 0; only scoped HTTP wire source/tests plus ADR/index lifecycle evidence changed; `Cargo.toml`, `Cargo.lock`, public exports, accepted contract files, golden fixtures, and unrelated source have no diff. | Command outputs, stable-symbol/diff review, fixture hashes, and tested commit SHA. | Pass | On tested commit `8fdc920` every command exits 0: fmt check OK; strict Clippy clean; the full suite is green across 21 test binaries (447 passed, 0 failed); governance tests 145/145 and validation passed. `git diff fb6329d..8fdc920` touches only `koduck-ai/src/adapters/http/wire.rs`, `koduck-ai/src/adapters/http/wire/tests.rs`, this ADR, and `docs/adr/INDEX.md`; `Cargo.toml`, `Cargo.lock`, public exports (`pub(super)` signatures unchanged), contract files, and fixtures show zero diff. |

Allowed final check statuses are `Pass`, `Fail`, or `N/A — <specific reason>`.
`Fail` blocks completion. `N/A` is valid only when the check's stated trigger or
precondition demonstrably does not apply.

## Completion Checklist [Required]

| ID | Item | Completion Criterion | Expected Evidence | Status | Actual Evidence |
| --- | --- | --- | --- | --- | --- |
| A-1 | ADR approved | An eligible non-author approver, approval time, and exact `Approval Evidence: Approve` are recorded; any optional Approval Context Revision is informational, non-binding, and exactly represents the approved document | ADR metadata | Complete | @linhai, the named Decision Owner and Required Approver, responded `Approve` in the ZCode task session that unambiguously identified this ADR path on 2026-08-25 |
| A-2 | Complete task delivered | Every declared subtask has actual implementation evidence, every applicable acceptance check is `Pass` with actual result and evidence, and together they satisfy the complete task outcome | Implementation Plan and Acceptance Checks rows | Complete | T-1, T-2, and T-3 are `Complete` and AC-1 through AC-4 are `Pass` on tested commit `8fdc920`: outbound JSON is serialized through private typed DTOs, buffered and live SSE share one item and one terminal mapping, and every public wire byte is unchanged. |
| A-3 | Reciprocal ADD link synchronized, when applicable | The selected candidate records this exact ADR path, this ADR records the exact ADD path and candidate ID, both references agree, and the candidate reaches `Complete` only when this ADR is `Complete` or `Verified` | Exact ADD path, candidate ID, ADR path, and Git blob or commit | N/A — not product demand | N/A — this corrective internal maintenance task selects no ADD candidate |
| A-4 | Requirement levels satisfied | Every required section is complete, every conditional trigger is assessed and completed or marked `N/A — <reason>`, and optional sections are complete or removed | Structured document review | Complete | Structured review on 2026-08-25 found no blank required content or unassessed conditional trigger; the remeasured Notes entry records the post-implementation decomposition review outcome. |
| A-5 | Acceptance checks are decidable | Every check names one subtask, preconditions or input, deterministic method, exact expected result, and evidence; no unqualified subjective criterion remains | Structured acceptance-check review | Complete | AC-1 through AC-4 name exact commands, preconditions, observable byte/state results, and evidence; each recorded an exact exit-0 result with per-fixture assertions on `8fdc920`. |
| A-6 | Engineering exceptions governed, when applicable | Every exceeded or waived engineering rule has one complete exception row, an accountable owner, a lifecycle, and verification evidence before approval; otherwise the conditional subsection records `N/A — <reason>` | Engineering Exceptions subsection and affected-file evidence | N/A — no exception planned | No engineering rule is exceeded or waived: the production file stayed below the 800-line exception limit after the test-module split recorded in Notes. |
| A-7 | Contract and baseline risks covered | Every normative contract clause maps to an explicit check or deterministic test, and every required Risk Coverage Matrix row is complete before approval and reaches Pass or specific N/A before review-ready or completion | Contract-To-Check Traceability, Risk Coverage Matrix, acceptance checks, and stable evidence | Complete | TW-01 through TW-06 map to AC-1 through AC-4; all five baseline dimensions have exact scenarios or N/A reasons with linked checks, and every applicable row is `Pass` on `8fdc920`. |
| A-8 | Governance validation passed | The independent validator reports no required-section, template-field, lifecycle-status, index, reciprocal-link, or Mermaid contract error for this record and repository | `npm run validate --prefix tools/governance-validator` output | Complete | Exit 0 on 2026-08-25 after the implementation evidence update; governance validation passed for the repository with this Accepted and Complete service ADR and central index row, and the validator test suite reported 145/145. |

## Notes [Optional]

- At source revision `fb6329d5f0231522a2b5011656f15f04cabab7fa`,
  `koduck-ai/src/adapters/http/wire.rs` is 500 physical lines: above the
  maintained-production-source decomposition-review threshold of 400, but
  below the 800-line engineering-exception limit.
- The observed size debt remains outside this task because this ADR owns one
  localized outbound-representation refactor and a broader module split would
  enlarge its implementation boundary and review surface. During
  implementation, remeasure the materially changed file and record the
  required decomposition review; if it remains above 400 lines, retain it only
  with evidence that the module still owns one cohesive HTTP wire capability
  and that extraction would worsen coupling, ordering visibility, or lifecycle
  review. Crossing 800 lines instead requires an approval-sensitive
  engineering exception before the source change proceeds.
- Remeasured on implementation commit `8fdc920`: the production file
  `koduck-ai/src/adapters/http/wire.rs` is 472 physical lines and the
  characterization tests live in `koduck-ai/src/adapters/http/wire/tests.rs`
  (561 lines, below the 600-line maintained-test-source review threshold) at
  module path `adapters::http::wire::tests`. An interim single-file state
  reached 1027 physical lines while the new tests were inlined; the test
  module was split into its own file before completion, so the production
  file stayed below the 800-line exception limit and no engineering exception
  was required. The production file remains above the 400-line review
  threshold and is retained because one module owns the single cohesive
  outbound HTTP wire capability — typed documents plus explicit SSE framing —
  and extracting any part would split one serialized contract across owners.

## Archival [Conditionally Required — Decision Status is `Rejected`, or Decision Status is `Deprecated` or `Superseded` and Implementation Status is final]

Archive this record in the same change that retires it when either Decision
Status is `Rejected` with Implementation Status `Not Applicable`, or Decision
Status is `Deprecated` or `Superseded` with Implementation Status `Verified`,
`Complete`, or `Not Applicable`. Before that trigger, retain this section as
inactive future-lifecycle guidance; its checklist does not affect acceptance or
implementation completion. When triggered:

- [ ] Move this file to `archive/ADR-0002-typed-http-wire-serialization.md`
      under this same ADR root.
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
| 2026-08-25 | Proposed one localized typed outbound-wire implementation that consolidates buffered/live mappings while preserving every existing public JSON and SSE byte contract. | @codex |
| 2026-08-25 | Expanded AC-2 to cover every terminal outcome across buffered/live paths, recorded the existing wire-module size debt and decomposition-review gate, and planned the required governed-file marker; no decision, public scope, contract, or implementation status changed. | @codex |
| 2026-08-25 | Accepted the record for development: @linhai responded `Approve`; Decision Status `Proposed` to `Accepted`, approval metadata recorded, and checklist A-1 satisfied. | @codex |
| 2026-08-25 | Implemented T-1 through T-3 on commits `db6c1f9`, `52a1ca3`, and `8fdc920`: characterization tests observed green before and after the typed-DTO refactor, shared item/terminal mappings wired into both SSE paths, tests split into `wire/tests.rs` to keep the production file below the exception limit, every acceptance check and routed command green; Implementation Status to `Complete`. | @codex |
