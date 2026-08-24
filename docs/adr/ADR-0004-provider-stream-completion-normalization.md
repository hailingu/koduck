# ADR-0004: Provider Stream Completion Normalization

## Metadata [Required]

- **Decision Status**: Accepted
- **Implementation Status**: Not Started
- **Date**: 2026-08-24
- **Author**: @codex
- **Decision Owner**: @linhai
- **Required Approver**: @linhai
- **Record Scope**: Project
- **Approver [Conditionally Required — Decision Status is or has been `Accepted`]**: @linhai
- **Approval Time [Conditionally Required — Decision Status is or has been `Accepted`]**: 2026-08-24T09:46:58Z
- **Approval Evidence [Conditionally Required — Decision Status is or has been `Accepted`]**: Approve
- **Rejector [Conditionally Required — Decision Status is `Rejected`]**: N/A — Decision Status is `Proposed`
- **Rejection Time [Conditionally Required — Decision Status is `Rejected`]**: N/A — Decision Status is `Proposed`
- **Rejection Evidence [Conditionally Required — Decision Status is `Rejected`]**: N/A — Decision Status is `Proposed`
- **Retired By [Conditionally Required — Decision Status is `Deprecated` or `Superseded`]**: N/A — Decision Status is `Proposed`; the record is active
- **Retirement Time [Conditionally Required — Decision Status is `Deprecated` or `Superseded`]**: N/A — Decision Status is `Proposed`; the record is active
- **Retirement Evidence [Conditionally Required — Decision Status is `Deprecated` or `Superseded`]**: N/A — Decision Status is `Proposed`; the record is active
- **Retirement Reason [Conditionally Required — Decision Status is `Deprecated` or `Superseded`]**: N/A — Decision Status is `Proposed`; the record is active
- **Blocked From [Conditionally Required — Implementation Status is `Blocked`]**: N/A — Implementation Status is `Not Started`
- **Blocker And Evidence [Conditionally Required — Implementation Status is `Blocked`]**: N/A — Implementation Status is `Not Started`
- **Blocker Owner [Conditionally Required — Implementation Status is `Blocked`]**: N/A — Implementation Status is `Not Started`
- **Blocker Exit Or Recheck Criterion [Conditionally Required — Implementation Status is `Blocked`]**: N/A — Implementation Status is `Not Started`
- **Related [Optional]**: `docs/adr/ADR-0001-provider-neutral-turn-kernel.md`; [MiniMax Codex configuration](https://platform.minimaxi.com/docs/token-plan/codex)
- **Architecture Source [Conditionally Required — product demand]**: N/A — this corrective provider-protocol compatibility task was discovered through local verification of the existing provider-neutral runtime and is not derived from a new Trello product requirement
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

The `koduck-ai` runtime translates one configured OpenAI-compatible Chat
Completions stream into provider-neutral application events. The current
adapter treats the literal SSE sentinel `data: [DONE]` as the only successful
Turn-completion signal. A stream that closes without that sentinel ends the
provider iterator without a `ProviderEvent::Completed`; the runner then commits
the typed failure `PROVIDER_STREAM_ENDED`, and synchronous delivery exposes
`503 provider-unavailable`.

On 2026-08-24 a redacted local compatibility probe against the configured
MiniMax M3 Chat Completions endpoint returned HTTP 200, ordered content deltas,
`finish_reason: "stop"`, and a final usage frame, then closed its HTTP body
cleanly without `data: [DONE]`. The credential and response content were not
retained in repository evidence. This proved that the reported failure was not
caused by the Base URL, model identifier, authentication, PostgreSQL, or the
northbound identity boundary. It also showed that copying a Codex-specific
`wire_api = "responses"` example into runtime configuration is neither
available nor necessary for the narrow compatibility defect: the configured
Chat Completions endpoint already returns a successful stream.

OpenAI-compatible providers vary in their stream-finalization details. Koduck
must accept a clean transport end only after explicit, validated protocol
terminal evidence, while continuing to reject truncated, timed-out, malformed,
or ambiguous streams. Provider hostnames, model names, or credential types must
not select completion semantics.

## Scope [Required]

In scope:

- Add one provider-neutral normalization rule for OpenAI-compatible Chat
  Completions streams terminated by either `data: [DONE]` or an explicit
  supported `finish_reason` followed by a clean transport end.
- Distinguish a clean HTTP response-body end from timeout, body-read failure,
  consumer cancellation, task/channel loss, and unannounced iterator end.
- Preserve ordered deltas, optional usage, Tool-call continuation, cumulative
  bounds, and existing typed failure behavior.
- Add focused protocol and production-transport regression tests before the
  implementation change, plus the minimum runtime-contract documentation.

Out of scope:

- Implementing the OpenAI Responses API or selecting a runtime wire API.
- Adding MiniMax-specific, Kimi-specific, OpenAI-specific, hostname-specific,
  model-specific, or credential-specific branches.
- Adding provider fallback, retry, routing, catalog, reasoning-control, or
  multimodal behavior.
- Changing the northbound REST/SSE v1 schema, identity handoff, PostgreSQL
  lifecycle, Tool/MCP policy, deployment, or runtime environment variables.
- Treating response-body truncation, network disconnect, timeout, malformed
  frames, or an unsupported finish reason as success.

## Tensions, Constraints, And Open Questions [Required]

### Identified Tensions [Conditionally Required — competing goals or trade-offs exist]

| ID | Tension | Impact | Decision |
| --- | --- | --- | --- |
| TN-1 | Provider compatibility versus fail-closed completion | Requiring only `[DONE]` rejects valid provider streams; accepting arbitrary EOF can commit truncated output as completed | Accept clean end only after the adapter has parsed one supported terminal `finish_reason`; every other end remains a failure |
| TN-2 | Shared protocol semantics versus vendor-specific exceptions | Host/model detection would make the core behavior depend on mutable provider branding and produce untestable compatibility branches | Normalize only wire-visible semantics inside the existing provider adapter and prohibit vendor identity checks |
| TN-3 | Narrow defect correction versus full Responses API support | Adding another wire API would expand configuration, request/response translation, tool semantics, and acceptance scope beyond one reviewable slice | Retain Chat Completions for this task; evaluate Responses API separately if product requirements require it |

### Constraints [Required]

- `docs/adr/ADR-0001-provider-neutral-turn-kernel.md` remains authoritative for
  provider-neutral core ownership, bounded provider execution, durable terminal
  ordering, and northbound failure mapping.
- `docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md` remains
  authoritative for Tool-call assembly, continuation, limits, and execution
  policy; a clean end after `finish_reason: "tool_calls"` ends only that model
  round and must not complete the Turn.
- `data: [DONE]` retains its existing meaning and compatibility behavior.
- Clean HTTP body completion must be represented explicitly by the production
  transport and ordered after every decoded data frame; channel disconnection
  or iterator exhaustion without that representation is not a clean end.
- Only `finish_reason: "stop"` authorizes Turn completion at clean end in this
  ADR. `finish_reason: "tool_calls"` authorizes Tool-round continuation.
  Missing, conflicting, repeated, or any other finish reason at clean end fails
  with a stable provider code and never emits `ProviderEvent::Completed`.
- A supported finish frame may be followed only by an optional valid usage
  frame and then `[DONE]` or explicit clean end. Later content, Tool fragments,
  errors, duplicate usage, or another finish reason fail closed.
- Existing response-header, stream-idle, total, frame-size, Tool-call count,
  Tool-argument size, item, and backpressure limits must not be widened.
- No new dependency, runtime variable, persisted schema, external operation, or
  governed build is authorized by this record.
- Every maintained source file changed by the implementation must cite
  `docs/adr/ADR-0004-provider-stream-completion-normalization.md` at its first
  legal comment position while retaining any still-applicable existing ADR
  markers.
- Source implementation must follow Red-Green-Refactor after this record is
  accepted; ADR drafting and governance validation remain documentation-only.

### Open Questions [Conditionally Required — material questions exist or were resolved during drafting]

| ID | Question | Owner | Due | Status | Resolution and Evidence |
| --- | --- | --- | --- | --- | --- |
| Q-1 | Is Responses API support required to make the observed MiniMax endpoint usable? | @linhai | 2026-08-24 | Resolved | No. The redacted 2026-08-24 probe to the configured `/v1/chat/completions` endpoint returned HTTP 200 and valid Chat Completions SSE; the failure occurred only after its clean no-`[DONE]` end. The MiniMax Codex guide's `wire_api = "responses"` is client-specific configuration and remains related context, not this task's selected protocol. |
| Q-2 | Which no-`[DONE]` finish reasons are successful? | @linhai | 2026-08-24 | Resolved | Only `stop` completes the Turn. `tool_calls` ends the current model round without Turn completion. Other, missing, conflicting, or repeated values fail closed until a separately accepted decision assigns them semantics. |
| Q-3 | May provider identity or runtime configuration choose the rule? | @linhai | 2026-08-24 | Resolved | No. The adapter uses only validated transport and frame semantics, so the same stream has the same result for every configured provider. |

## Decision Drivers [Required]

1. **Fail-closed correctness**: A truncated or ambiguous model response must not
   become a durable completed Turn.
2. **Provider neutrality**: Compatibility must be based on protocol evidence,
   not provider, hostname, model, or credential identity.
3. **Backward compatibility**: Existing `[DONE]` streams and Tool-call
   continuation behavior must retain their accepted results.
4. **One reviewable slice**: The decision must stay inside the provider
   integration boundary and be deliverable through one implementation pull
   request.
5. **Deterministic verification**: Every accepted terminal and rejected edge
   must be reproducible without a live third-party credential.

## Options Considered [Required]

### Option: Require `data: [DONE]` for every successful stream

Keep the current parser unchanged and require operators to choose only
providers that emit the OpenAI sentinel.

Pros:

- No implementation change.
- Keeps one unmistakable successful terminator.

Cons:

- Rejects an otherwise valid ordered stream after a provider reports `stop`,
  supplies usage, and closes cleanly.
- Makes the broad “OpenAI-compatible” configuration misleading without an
  enforceable startup capability check.

### Option: Add provider-specific completion modes

Add a provider name or completion-mode setting and select vendor-specific EOF
rules at runtime.

Pros:

- Can model provider quirks independently.
- Makes each configured mode explicit to operators.

Cons:

- Adds configuration and vendor routing without a need for different owned
  semantics.
- Risks identical wire streams receiving different outcomes and requires
  continuous vendor-specific maintenance.

### Option: Normalize explicit protocol terminal evidence

Extend the existing transport/frame state machine with an explicit clean-end
event. Preserve `[DONE]`; additionally complete a Turn when a validated `stop`
finish frame is followed only by optional valid usage and explicit clean end.
End a `tool_calls` round without Turn completion under the same clean-end rule.

Pros:

- Supports the observed provider variation without vendor detection.
- Distinguishes success from timeout, body failure, cancellation, and silent
  channel loss.
- Keeps translation inside the provider adapter and the core provider-neutral.

Cons:

- Adds terminal state to the parser and transport contract.
- Requires strict ordering tests so post-finish output cannot be silently
  accepted.

### Option: Replace Chat Completions with Responses API

Add Responses API configuration, request translation, stream event parsing,
Tool semantics, and runtime selection now.

Pros:

- Aligns directly with the cited MiniMax Codex configuration.
- Creates a future path for Responses-specific reasoning and Tool events.

Cons:

- Does not represent a narrow fix for a Chat Completions endpoint that already
  returned HTTP 200.
- Crosses request, response, Tool, configuration, and compatibility concerns
  and cannot be delivered as this one provider-boundary slice.

## Decision [Required]

**Selected option**: Normalize explicit protocol terminal evidence.

**Rationale**: The selected option accepts the demonstrated variation using
only evidence owned by the existing protocol adapter, while keeping transport
failure distinct from clean completion. It preserves the provider-neutral core,
does not add a vendor registry or wire-API selection contract, and can be proven
through deterministic frames plus a local HTTP transport harness in one pull
request.

### Normalized Completion Contract [Required]

- **PSC-1 — Explicit clean end**: The production transport must emit exactly
  one ordered clean-end frame only after the HTTP response body reaches a
  successful EOF and every buffered byte has been decoded. A response-header
  timeout, stream-idle timeout, total timeout, body-read error, oversized frame,
  consumer cancellation, task/channel loss, or undecodable trailing bytes must
  emit or retain a failure outcome and must not emit clean end.
- **PSC-2 — OpenAI sentinel compatibility**: `data: [DONE]` retains the existing
  behavior: it completes a non-Tool stream, ends a served Tool-call round
  without completing the Turn, and rejects unfinished Tool-call fragments.
  The sentinel is terminal evidence by itself and does not require an earlier
  `finish_reason`; implementation must not add that prerequisite to the
  existing sentinel path.
- **PSC-3 — Stop plus clean end**: After exactly one validated
  `finish_reason: "stop"`, the parser may accept at most one valid usage frame
  and then explicit clean end. It must emit exactly one
  `ProviderEvent::Completed`; the runner must preserve durable append-before-
  publish ordering and the existing northbound completed response. Unlike
  `[DONE]`, clean end is not terminal evidence by itself and must never
  complete a Turn without the preceding supported finish reason.
- **PSC-4 — Tool round plus clean end**: After exactly one validated
  `finish_reason: "tool_calls"`, the parser must flush every fully assembled
  Tool call in index order. A following optional valid usage frame and explicit
  clean end end that model round without emitting `ProviderEvent::Completed`,
  so the runner continues with committed Tool results under ADR-0003.
- **PSC-5 — Ambiguous or late output**: Explicit clean end without a supported
  finish reason or after an unsupported finish reason must emit the newly
  declared `OPENAI_UNEXPECTED_EOF`. Repeated or conflicting finish reasons and
  content/Tool/error output after a finish frame must emit the newly declared
  `INVALID_FINISH_FRAME`; duplicate or otherwise invalid post-finish usage must
  retain the existing `DUPLICATE_USAGE_FRAME` or `INVALID_USAGE_FRAME`;
  unfinished Tool-call fragments must retain the existing
  `INVALID_TOOL_CALL_FRAME`. Every case must emit zero
  `ProviderEvent::Completed` events.
- **PSC-6 — Provider-neutral selection**: Completion behavior must not inspect
  provider hostname, model, credential, tenant, subject, or runtime environment
  beyond the existing OpenAI-compatible transport configuration.
- **PSC-7 — Northbound compatibility**: A normalized completed Turn retains the
  existing `200` synchronous and `turn.completed` SSE contracts. Every rejected
  provider termination retains the existing durable failed Turn and
  `provider-unavailable` delivery mapping; no public JSON or SSE field changes.

### Consequences [Required]

Positive:

- Providers that close cleanly after an explicit `stop` can complete without
  pretending to be a named vendor or adopting another wire API.
- Transport EOF and protocol completion become independently testable.
- Truncated and ambiguous streams remain typed failures.

Negative:

- The provider adapter gains one explicit transport-end variant and terminal
  parser state.
- Finish reasons other than `stop` and `tool_calls` remain unsupported for
  clean-end completion even when a provider treats them as terminal.

Mitigations:

- Keep the recognized set deliberately small and require a separately accepted
  decision before assigning success semantics to another finish reason.
- Prove both the deterministic parser and real Reqwest body-end boundary, while
  preserving the routed timeout, cancellation, and resource-bound suites.

## Implementation Plan [Required]

**Complete task outcome**: One independently reviewable implementation pull
request makes OpenAI-compatible `[DONE]` streams and explicit `stop` plus clean-
end streams produce the same provider-neutral completed Turn, makes clean end
after `tool_calls` continue rather than complete the Turn, and deterministically
rejects every declared malformed, unsupported, truncated, timed-out, cancelled,
or out-of-order terminal case without changing the northbound wire contract.

**Primary implementation boundary**: Provider and transport integration —
`koduck_ai::adapters::provider`.

Allowed subtask statuses: `Not Started`, `In Progress`, `Blocked`, `Complete`,
or `N/A — <specific reason>`.

| ID | Objective or deliverable | Included scope | Status | Actual implementation evidence |
| --- | --- | --- | --- | --- |
| T-1 | Implement and document the normalized completion state machine test-first | Explicit clean transport-end framing, supported finish-state parsing, `[DONE]` compatibility, Tool-round continuation, typed rejection, focused protocol/Reqwest/runtime regressions, and the minimum runtime contract copy | Not Started | Not run — implementation requires this ADR to become `Accepted` |

**Affected paths**: `koduck-ai/src/adapters/provider/mod.rs`,
`koduck-ai/src/adapters/provider/stream_state.rs`,
`koduck-ai/tests/openai_provider.rs`,
`koduck-ai/tests/provider_stream_lifecycle.rs`, the narrowest existing
runner/HTTP integration test needed to prove PSC-7, and
`koduck-ai/docs/runtime-configuration.md`. No new dependency, migration,
runtime variable, or generated artifact is expected.

### Stable Implementation Touchpoints [Conditionally Required — source or configuration implementation]

| Path | Stable symbol or contract anchor | Key code excerpt, when needed | Purpose | Source revision |
| --- | --- | --- | --- | --- |
| `koduck-ai/src/adapters/provider/mod.rs` | `OpenAiFrame`, `ReqwestOpenAiTransport::chat_completion_frames`, `pump_request`, `pump_response` | N/A — stable symbols identify the production transport and frame boundary | Own explicit ordered clean-end delivery and keep all non-clean transport endings typed as failures | `2d0f0dfb86411b4dc8648cd7dbb74241c3be620f` |
| `koduck-ai/src/adapters/provider/stream_state.rs` | `StreamState::next_event`, `StreamState::parse_frame`, `StreamState::flush_tool_calls` | N/A — stable symbols identify terminal parsing and Tool-round ownership | Normalize `[DONE]`, supported finish reasons, usage ordering, clean end, and fail-closed terminal conflicts | `2d0f0dfb86411b4dc8648cd7dbb74241c3be620f` |
| `koduck-ai/src/application/runner.rs` | `run_accepted`, `drive_stream`, `handle_event` | N/A — stable symbols identify the provider-neutral consumer | Preserve existing completed, Tool-continuation, and `PROVIDER_STREAM_ENDED` fallback behavior; supporting changes are permitted only if the adapter cannot prove PSC-7 without them | `2d0f0dfb86411b4dc8648cd7dbb74241c3be620f` |
| `koduck-ai/docs/runtime-configuration.md` | `Required Environment`, `Startup`, `Operational Bounds` | N/A — stable headings identify the runtime contract copy | Document accepted Chat Completions completion variants without adding configuration or exposing credentials | `2d0f0dfb86411b4dc8648cd7dbb74241c3be620f` |

**Migration and rollback strategy [Conditionally Required — this replaces or
changes existing behavior]**: Forward migration changes no data, environment,
endpoint, or deployment contract. After acceptance, merge the test-first source
and documentation change in one implementation pull request. Stop if any
timeout, cancellation, malformed-frame, Tool-call, usage-ordering, or existing
`[DONE]` regression fails, or if a clean end can be produced after a non-clean
transport outcome. Before any promotion, rollback is source reversion of that
implementation commit, restoring `[DONE]`-only completion; existing durable
Turns require no migration. Any retained build, release, deployment, or runtime
operation remains separately governed and is not authorized here.

### Engineering Exceptions [Conditionally Required — an engineering rule is exceeded or waived]

N/A — no engineering rule is exceeded or waived. At source revision
`2d0f0dfb86411b4dc8648cd7dbb74241c3be620f`,
`koduck-ai/src/adapters/provider/mod.rs` is 559 physical lines and therefore
crosses the 400-line decomposition-review threshold but remains below the
800-line exception limit. The planned change is restricted to its cohesive
transport/frame responsibility and colocated transport tests; implementation
must remeasure the affected units and record a decomposition review without
using a mechanical split or a new exception.

## Contract-To-Check Traceability [Conditionally Required — source or configuration implementation]

| Clause ID | Authoritative contract path and heading | Exact normative requirement | Acceptance check or deterministic test IDs | Explicit coverage method |
| --- | --- | --- | --- | --- |
| PSC-1 | `docs/adr/ADR-0004-provider-stream-completion-normalization.md` — Normalized Completion Contract | Clean end is emitted exactly once and only after successful decoded body EOF; every enumerated non-clean ending is not clean end | AC-2, AC-6 | Local HTTP transport fixtures exercise decoded EOF, partial trailing data, body failure, timeout, and consumer drop, and assert the exact frame/error sequence |
| PSC-2 | `docs/adr/ADR-0004-provider-stream-completion-normalization.md` — Normalized Completion Contract | Existing `[DONE]` completion, Tool-round end, and unfinished-Tool rejection remain unchanged | AC-1, AC-4 | Deterministic frames replay the existing sentinel variants and compare exact owned event sequences |
| PSC-3 | `docs/adr/ADR-0004-provider-stream-completion-normalization.md` — Normalized Completion Contract | One `stop`, optional valid usage, and explicit clean end emit exactly one completed event and preserve the completed Turn outcome | AC-1, AC-5 | Deterministic and runner-level fixtures assert delta/usage/completed ordering and exact synchronous/SSE completion status |
| PSC-4 | `docs/adr/ADR-0004-provider-stream-completion-normalization.md` — Normalized Completion Contract | One `tool_calls` finish and explicit clean end flush Tool calls and end only the model round | AC-4 | Tool fragments and clean end assert index-ordered Tool events, zero completed events for that round, and one continuation request |
| PSC-5 | `docs/adr/ADR-0004-provider-stream-completion-normalization.md` — Normalized Completion Contract | Missing or unsupported finish at clean end emits `OPENAI_UNEXPECTED_EOF`; repeated/conflicting or late output emits `INVALID_FINISH_FRAME`; invalid usage and unfinished Tool fragments retain their enumerated existing errors; every case emits zero completion events | AC-3 | A table-driven deterministic fixture asserts the exact declared error code and zero completed events for every enumerated malformed sequence |
| PSC-6 | `docs/adr/ADR-0004-provider-stream-completion-normalization.md` — Normalized Completion Contract | Completion selection does not inspect vendor, host, model, credential, tenant, subject, or environment | AC-7 | Structured source review plus tests run identical frame fixtures through differently labelled model/input values and assert identical events |
| PSC-7 | `docs/adr/ADR-0004-provider-stream-completion-normalization.md` — Normalized Completion Contract | Normalized completion retains existing northbound success; rejected termination retains provider-failure delivery with no wire-field change | AC-5, AC-7 | Existing golden-contract tests plus one runner/HTTP regression assert exact status/event class and unchanged fixture hashes |

## Risk Coverage Matrix [Conditionally Required — source or configuration implementation]

| Risk dimension | Applicability and scenario, or specific N/A reason | Owning boundary | Deterministic verification method | Exact expected result | Acceptance check IDs | Status | Actual evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Concurrency and ordering | The asynchronous Reqwest pump could publish clean end before the final data/usage frame or more than once | Provider transport channel and `StreamState` | Local HTTP fixture sends split/chunked terminal and usage bytes; collect the exact owned sequence | Every decoded data frame precedes exactly one clean-end frame; owned events remain delta, optional usage, then exactly one completed event | AC-2 | Not Started | Not run — implementation not started |
| Timeout and deadline | Header, idle, or total timeout after partial output could be misclassified as clean completion | `pump_request`, `pump_response`, provider timing | Execute deterministic short-deadline header/idle/total timeout fixtures, including a partial `stop` frame before the timeout where applicable | The exact typed timeout error is emitted; clean end and completed events are absent | AC-6 | Not Started | Not run — implementation not started |
| Cancellation and interruption | Dropping the downstream stream or interrupting consumption could let sender/channel closure synthesize completion | Provider stream ownership and runner cancellation boundary | Drop a connected provider stream and execute the existing controlled cancellation/interrupt regressions | Upstream consumption is cancelled; no clean-end or completed event is synthesized; the owned cancellation/interruption terminal remains canonical | AC-6 | Not Started | Not run — implementation not started |
| Resource bounds and backpressure | A final oversized or unterminated frame, accumulated Tool arguments, or a full bounded channel could bypass limits when clean end is added | Provider frame buffer, bounded channel, Tool-call assembly | Run exact/over frame-size, Tool-call count/argument, and bounded-channel regression fixtures followed by EOF | Existing exact limits remain unchanged; over-limit input emits its existing typed error and never emits clean end or completion | AC-6 | Not Started | Not run — implementation not started |
| Framework or trust-boundary rejection | Non-success HTTP, malformed SSE/JSON, unsupported finish reason, or invalid runtime endpoint could be accepted as completion | Reqwest/provider protocol and runtime configuration boundary | Run production transport HTTP-status/body fixtures, malformed deterministic frames, and existing configuration regressions | Each input produces its existing or newly declared stable provider/configuration error, zero completed events, and no change to identity handling | AC-3, AC-7 | Not Started | Not run — implementation not started |

## Acceptance Checks [Required]

| Check ID | Subtask | Binary acceptance point | Preconditions or input | Verification method | Exact expected result | Expected evidence | Status | Actual result and evidence |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| AC-1 | T-1 | Sentinel and clean-end stop variants map to the same completed owned sequence | Deterministic streams contain the same delta and optional usage; one ends with `[DONE]`, the other has exactly one `finish_reason: "stop"` followed by explicit clean end | Run `cargo test -p koduck-ai --test openai_provider completion_variants_map_to_the_same_owned_events -- --exact` | Exit 0; both variants emit the same ordered Delta, optional Usage, and exactly one Completed event | Command output and exact event assertions | Not Started | Not run — implementation not started |
| AC-2 | T-1 | Production transport exposes clean EOF exactly once after all frames | Local chunked HTTP server ends successfully after split terminal/usage frames without `[DONE]` | Run `cargo test -p koduck-ai --test provider_stream_lifecycle reqwest_clean_eof_is_ordered_after_decoded_frames -- --exact` | Exit 0; all Data frames precede exactly one clean-end frame and the stream then ends without another frame or error | Command output and captured frame sequence | Not Started | Not run — implementation not started |
| AC-3 | T-1 | Every ambiguous or invalid clean-end sequence emits its declared typed error without completion | Table includes no finish reason, unsupported/repeated/conflicting finish reasons, unfinished Tool fragments, and content/error/Tool/duplicate usage after finish | Run `cargo test -p koduck-ai --test openai_provider invalid_clean_end_sequences_fail_closed -- --exact` | Exit 0; missing/unsupported finish emits `OPENAI_UNEXPECTED_EOF`; repeated/conflicting or late content/error/Tool output emits `INVALID_FINISH_FRAME`; duplicate/invalid usage emits `DUPLICATE_USAGE_FRAME`/`INVALID_USAGE_FRAME`; unfinished Tool fragments emit `INVALID_TOOL_CALL_FRAME`; every case emits zero Completed events | Command output and per-case error/event assertions | Not Started | Not run — implementation not started |
| AC-4 | T-1 | Clean end after a Tool-call finish ends only the model round | One complete indexed Tool-call sequence ends with `finish_reason: "tool_calls"` and clean end; runner fixture supplies a completed continuation stream | Run `cargo test -p koduck-ai --test cand_2_runner_tools clean_eof_tool_round_continues_once -- --exact` | Exit 0; first round emits ToolCall and zero Completed events, exactly one continuation request carries the committed result, and that continuation supplies the sole Turn completion | Command output, provider input count, and terminal assertions | Not Started | Not run — implementation not started |
| AC-5 | T-1 | Normalized completion preserves the public completed response and provider-failure mapping | Runner/HTTP fixture executes a stop-plus-clean-end stream and an unannounced-end control; existing golden fixtures are present | Run `cargo test -p koduck-ai --test cand_1_contract provider_completion_normalization_preserves_v1_delivery -- --exact` | Exit 0; normalized completion produces the exact existing completed status/event class, control produces provider failure, and all three existing golden fixture hashes remain unchanged | Command output, HTTP/SSE assertions, and fixture-hash assertions | Not Started | Not run — implementation not started |
| AC-6 | T-1 | Timeout, cancellation, and resource-bound endings never become clean completion | Existing header/idle/total timeout, disconnect, exact/over frame-size, Tool count/arguments, and controlled cancellation fixtures | Run `cargo test -p koduck-ai --test provider_stream_lifecycle`; run `cargo test -p koduck-ai --test openai_provider`; run `cargo test -p koduck-ai --test cand_1_liveness` | All commands exit 0; each enumerated failure retains its exact typed outcome and emits zero clean-end/Completed events; existing exact-limit cases remain accepted | Command outputs and named regression results | Not Started | Not run — implementation not started |
| AC-7 | T-1 | The complete routed implementation and governance checks pass without vendor branches, provenance gaps, or contract drift | Accepted implementation is present; no live credential is required; provider-neutral source and documentation are updated | Run `cargo fmt --all --check`; run `cargo clippy -p koduck-ai --all-targets --all-features -- -D warnings`; run `cargo test -p koduck-ai --all-targets --all-features`; run `npm test --prefix tools/governance-validator`; run `npm run validate --prefix tools/governance-validator`; inspect the implementation diff for provider/model/host/credential branching and required governed-file markers, then remeasure affected units | Every command exits 0; structured inspection finds zero completion decisions based on provider/model/host/credential identity; every changed maintained source file cites this ADR at its first legal comment position while retaining applicable existing markers; no public fixture change, new dependency/configuration/migration, or affected unit above an unapproved exception limit exists | Command outputs, structured diff review, governed-file marker inspection, unit measurements, and tested commit SHA | Not Started | Not run — implementation not started |

Allowed final check statuses are `Pass`, `Fail`, or `N/A — <specific reason>`.
`Fail` blocks completion. `N/A` is valid only when the check's stated trigger or
precondition demonstrably does not apply.

## Completion Checklist [Required]

| ID | Item | Completion Criterion | Expected Evidence | Status | Actual Evidence |
| --- | --- | --- | --- | --- | --- |
| A-1 | ADR approved | An eligible non-author approver, approval time, and exact `Approval Evidence: Approve` are recorded; any optional Approval Context Revision is informational, non-binding, and exactly represents the approved document | ADR metadata | Complete | @linhai self-identified in the task conversation and responded `Approve` for this exact record on 2026-08-24; Approver, Approval Time (2026-08-24T09:46:58Z), and `Approval Evidence: Approve` are recorded in Metadata |
| A-2 | Complete task delivered | T-1 has actual implementation evidence, AC-1 through AC-7 are Pass, and together they satisfy the complete task outcome | Implementation Plan and Acceptance Checks rows | Not Started | Pending — implementation not authorized |
| A-3 | Reciprocal ADD link synchronized, when applicable | The selected candidate records this exact ADR path, this ADR records the exact ADD path and candidate ID, both references agree, and the candidate reaches `Complete` only with this ADR's `Complete` or `Verified` status | Exact ADD path, candidate ID, ADR path, and Git blob or commit | N/A — not product demand | This corrective protocol task is not derived from product demand and selects no ADD candidate |
| A-4 | Requirement levels satisfied | Every required section is complete, every conditional trigger is assessed and completed or marked `N/A — <reason>`, and optional sections are complete or removed | Structured document review | Complete | Structured review on 2026-08-24 found every required section present, every conditional trigger assessed, retained optional content complete, and zero unresolved template placeholders |
| A-5 | Acceptance checks are decidable | Every check names T-1, preconditions or input, deterministic method, exact expected result, and evidence; no unqualified subjective criterion remains | Structured acceptance-check review | Complete | Structured review on 2026-08-24 found seven checks; each names T-1, exact inputs, an executable method, an observable expected result, and evidence, with PSC-5 error codes enumerated |
| A-6 | Engineering exceptions governed, when applicable | Every exceeded or waived engineering rule has one complete exception row, an accountable owner, a lifecycle, and verification evidence before approval; otherwise the conditional subsection records `N/A — <reason>` | Engineering Exceptions subsection and affected-file evidence | N/A — no exception proposed | No engineering exception is proposed; the existing decomposition-review threshold is assessed separately and must be remeasured during implementation |
| A-7 | Contract and baseline risks covered, when applicable | PSC-1 through PSC-7 map to explicit checks, and all five baseline risk rows are complete before approval and Pass before review-ready or completion | Contract-To-Check Traceability, Risk Coverage Matrix, acceptance checks, and stable evidence | Not Started | Pending — mappings are drafted; implementation evidence is not yet available |
| A-8 | Governance validation passed | The independent validator reports no required-section, template-field, lifecycle-status, index, reciprocal-link, or Mermaid contract error for this record and repository | `npm run validate --prefix tools/governance-validator` output | Complete | On 2026-08-24, `npm test --prefix tools/governance-validator` passed 145 tests with zero failures and `npm run validate --prefix tools/governance-validator` reported no error after ADR/index synchronization |

## Supporting Notes [Optional]

- This ADR assigns no normative behavior to a live third-party response body;
  the observed MiniMax response motivates deterministic fixtures, which become
  the implementation evidence after acceptance.
- Responses API support remains a possible future Full ADR concern because it
  would add a distinct request/stream protocol and runtime selection contract.
- `run.sh` and its credential are local operator inputs, are not affected paths,
  and must not be committed or copied into evidence.

## Archival [Conditionally Required — Decision Status is `Rejected`, or Decision Status is `Deprecated` or `Superseded` and Implementation Status is final]

Archive this record in the same change that retires it when either Decision
Status is `Rejected` with Implementation Status `Not Applicable`, or Decision
Status is `Deprecated` or `Superseded` with Implementation Status `Verified`,
`Complete`, or `Not Applicable`. Before that trigger, retain this section as
inactive future-lifecycle guidance; its checklist does not affect acceptance or
implementation completion. When triggered:

- [ ] Move this file to `archive/ADR-0004-provider-stream-completion-normalization.md`
      under this project ADR root.
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
| 2026-08-24 | Accepted: Decision Status `Proposed` → `Accepted` after @linhai's exact `Approve` response identifying this record; Implementation Status remains `Not Started`. | @linhai |
| 2026-08-24 | Made the intentional `[DONE]` versus clean-end evidence asymmetry explicit and added the implementation-time governed-file marker requirement to the constraints and routed acceptance check. | @codex |
| 2026-08-24 | Clarified that `OPENAI_UNEXPECTED_EOF` and `INVALID_FINISH_FRAME` are newly declared diagnostics, while the usage and Tool-frame diagnostics named by PSC-5 already exist. | @codex |
| 2026-08-24 | Proposed one provider-integration slice that normalizes explicit Chat Completions terminal evidence across `[DONE]` and clean-end variants while keeping ambiguous, truncated, and vendor-selected behavior fail-closed. | @codex |
