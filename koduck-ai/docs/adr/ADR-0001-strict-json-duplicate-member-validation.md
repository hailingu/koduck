# ADR-0001: Strict JSON Duplicate-Member Validation

## Metadata [Required]

- **Decision Status**: Accepted
- **Implementation Status**: Complete
- **Date**: 2026-08-25
- **Author**: @codex
- **Decision Owner**: @linhai
- **Required Approver**: @linhai
- **Record Scope**: Service internal — koduck-ai
- **Approver [Conditionally Required — Decision Status is or has been `Accepted`]**: @linhai
- **Approval Time [Conditionally Required — Decision Status is or has been `Accepted`]**: 2026-08-25T01:34:22Z
- **Approval Evidence [Conditionally Required — Decision Status is or has been `Accepted`]**: Approve
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
- **Related [Optional]**: `docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md`; `docs/adr/ADR-0004-provider-stream-completion-normalization.md`
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
  implementation, completion, or verification. If retained, it MUST be
  accurate and complete; optional content MUST NOT substitute for required
  evidence.

Unlabeled fields inside a `[Required]` section are required.

## Context And Problem Statement [Required]

`koduck-ai` has two security-sensitive consumers that must reject duplicate
JSON object members before `serde_json::Value` can collapse them with
last-member-wins behavior. The Tool adapter validates native Tool parameters,
MCP arguments, and descriptor JSON Schema in
`koduck-ai/src/adapters/tool.rs`; the OpenAI-compatible provider state machine
validates every provider data frame in
`koduck-ai/src/adapters/provider/stream_state.rs` before trusting terminal
evidence. Both consumers currently carry an almost identical private
`UniqueJson` deserializer and recursive visitor.

The duplicated implementation is currently behaviorally correct and its
consumer contracts have focused regression coverage. It nevertheless creates
two owners for the same fail-closed security rule: a future change to supported
serde scalar visits, nested-member handling, malformed-input behavior, or
diagnostics could update one boundary without the other. The duplication was
introduced deliberately when ADR-0004's affected-path scope could not authorize
a shared maintained source file; that completed record must not be expanded
retroactively. This new record owns the independently reviewable extraction and
the evidence that both existing consumer-visible outcomes remain unchanged.

## Scope [Required]

In scope:

- Add one private, purpose-specific `adapters::strict_json` capability that
  recursively rejects duplicate object members in one bounded JSON document.
- Replace the Tool adapter's private duplicate-member visitor with the shared
  validator while retaining Tool-owned size checks and exact error mappings.
- Replace the provider stream state's private duplicate-member visitor with the
  shared validator while retaining provider-owned frame bounds, state
  transitions, and exact error mappings.
- Add focused shared-validator characterization tests and retain the existing
  Tool and provider consumer regressions.

Out of scope:

- HTTP response serialization, typed wire DTOs, JSON field ordering, SSE
  framing, public request or response schema changes, and golden-fixture
  changes.
- Changes to accepted Tool/MCP parameter or schema semantics, provider
  completion semantics, error codes, resource limits, or canonicalization.
- A repository-wide generic JSON utility, a new crate, or a new dependency.
- Implementation of the later HTTP typed-wire maintenance task.

## Tensions, Constraints, And Open Questions [Required]

### Identified Tensions [Conditionally Required — competing goals or trade-offs exist]

| ID | Tension | Impact | Decision |
| --- | --- | --- | --- |
| TN-1 | One security-rule owner reduces drift, while consumer-local code keeps each trust boundary self-contained. | Retaining two visitors permits semantic divergence; an overly broad shared helper obscures ownership and error mapping. | Share only syntax-level duplicate-member rejection in `adapters::strict_json`; keep bounds, domain translation, state transitions, and typed errors in each consumer. |
| TN-2 | Extraction reduces two large source files, while generic shared utility modules are prohibited. | A generic `utils` or `json` module could become an unowned dumping ground. | Name the module after its single strict-validation capability, expose one crate-private operation, and reject unrelated JSON helpers from this scope. |

### Constraints [Required]

- ADR-0003's Tool/MCP duplicate-aware, size-bounded, fail-closed translation
  remains authoritative.
- ADR-0004 PSC-3, PSC-5, and PSC-7 remain authoritative for provider terminal
  evidence, exact typed rejection, and northbound compatibility.
- The shared validator must remain inside `koduck-ai`'s adapter boundary; no
  serialization or provider type may leak into application or domain modules.
- Tool action-input and descriptor-schema byte limits, and provider frame
  bounds, must be enforced by their existing owners before or around strict
  parsing exactly as they are at source revision
  `d396c8c99916d25c2fbd47663455496761ac3add`.
- Consumer-visible error mappings must remain exact: duplicate action
  parameters map to `ToolAdapterError::InvalidJson`, duplicate descriptor
  schema members map to `ToolAdapterError::InvalidSchema`, and duplicate
  provider-frame members map to provider code `INVALID_FRAME`.
- The implementation must not add a dependency or create a public API.

### Open Questions [Conditionally Required — material questions exist or were resolved during drafting]

None — no material questions. The owner, visibility, error ownership, resource
bounds, and compatibility requirements are resolved by this decision.

## Decision Drivers [Required]

1. **Fail-closed consistency**: Every untrusted JSON boundary using this rule
   must reject duplicate members at every nesting depth before semantic parsing.
2. **Single ownership**: The recursive serde visitor must have one maintained
   implementation and one focused test surface.
3. **Consumer-owned semantics**: Tool and provider adapters must retain their
   distinct bounds, error types, and lifecycle decisions.
4. **Minimal dependency and visibility surface**: The change must remain
   crate-private and use the existing `serde` and `serde_json` dependencies.
5. **Reviewable compatibility**: Existing contract tests must demonstrate no
   observable Tool, provider, HTTP, or SSE behavior change.

## Options Considered [Required]

### Option: Retain both private visitors

Keep the current implementations in `adapters/tool.rs` and
`adapters/provider/stream_state.rs`.

Pros:

- No source change and complete consumer locality.
- Existing accepted-record affected paths remain untouched.

Cons:

- Two owners must remain manually synchronized for one security rule.
- Future serde visitor changes can drift without a shared conformance point.
- `stream_state.rs` remains above the 400-line decomposition-review threshold
  partly because it embeds a second copy of the validator.

### Option: Add one purpose-specific adapter validator

Create `adapters::strict_json` with one crate-private duplicate-member
validation operation, focused tests, and consumer-owned error conversion at
each call site.

Pros:

- Establishes one implementation and one focused syntax-validation boundary.
- Preserves Tool and provider ownership of resource bounds and typed errors.
- Removes duplicated visitor code without adding a dependency or public API.

Cons:

- Both consumers depend on a shared adapter-internal module.
- A defect in the shared validator affects both trust boundaries at once.

### Option: Introduce an external strict-JSON dependency or general JSON facade

Replace the visitor with another parser crate or route parsing through a broad
adapter JSON abstraction.

Pros:

- Could provide additional strict-parsing features beyond duplicate-member
  rejection.

Cons:

- Adds dependency, supply-chain, and compatibility scope not required by the
  demonstrated problem.
- A general facade would mix parsing, bounds, translation, and provider state
  responsibilities that currently have distinct owners.

## Decision [Required]

**Selected option**: Add one purpose-specific adapter validator.

**Rationale**: The two consumers already implement the same recursive syntax
policy and are expected to evolve together for that policy. A single private
validator removes the demonstrated duplication while preserving the more
important ownership boundaries: the Tool adapter continues to own size limits,
schema/action translation, and `ToolAdapterError`; the provider state machine
continues to own frame lifecycle, completion evidence, and `ProviderError`.
The narrow module avoids a generic utility surface and requires no dependency
or public protocol change.

### Normative Contract Clauses [Required]

- **SJ-01 — Recursive duplicate rejection**: The shared validator must reject
  a JSON document when any object at any nesting depth contains the same member
  name more than once.
- **SJ-02 — Valid and malformed input**: The shared validator must accept every
  syntactically valid JSON document without duplicate object members and must
  return an error for malformed JSON.
- **SJ-03 — Tool mapping compatibility**: Tool action parameters containing a
  duplicate member must remain `ToolAdapterError::InvalidJson`; descriptor JSON
  Schema containing a duplicate member must remain
  `ToolAdapterError::InvalidSchema`; their existing byte limits must be checked
  before semantic translation.
- **SJ-04 — Provider mapping compatibility**: A provider frame containing a
  duplicate object member, including duplicate top-level `choices` or nested
  `finish_reason`, must remain provider code `INVALID_FRAME` and emit zero
  `ProviderEvent::Completed` events.
- **SJ-05 — Boundary compatibility**: The implementation must add no public
  symbol, dependency, public JSON/SSE field, accepted input, error code, state
  transition, or resource-limit change.

### Consequences [Required]

Positive:

- Duplicate-member rejection has one implementation and focused owner.
- Tool and provider regressions exercise the same strict parsing capability.
- The Tool and provider state modules lose duplicated visitor code.

Negative:

- One shared defect can affect both security-sensitive consumers.
- Future consumer-specific syntax needs cannot be added to the shared module
  without revisiting whether the semantics still match.

Mitigations:

- Keep the shared API to one duplicate-member operation and retain independent
  end-to-end error-mapping tests at both consumers.
- Exercise valid scalars, arrays, nested objects, malformed JSON, and duplicate
  names at multiple nesting depths in the shared validator's focused tests.
- Treat any future accepted-input, error, bound, or public-contract change as
  approval-invalidating and reclassify it before implementation.

## Implementation Plan [Required]

**Complete task outcome**: One independently reviewable `koduck-ai` source
change establishes a single private recursive duplicate-member validator used
by the Tool and provider adapters, with all SJ-01 through SJ-05 checks passing
and no consumer-visible behavior change.

**Primary implementation boundary**: `koduck_ai::adapters::strict_json`, the
adapter-internal syntax-validation owner for recursive duplicate-member
rejection.

Allowed subtask statuses: `Not Started`, `In Progress`, `Blocked`, `Complete`,
or `N/A — <specific reason>`.

| ID | Objective or deliverable | Included scope | Status | Actual implementation evidence |
| --- | --- | --- | --- | --- |
| T-1 | Add and characterize the private strict-JSON validator. | `koduck-ai/src/adapters/strict_json.rs`; private module declaration; focused unit tests for SJ-01/SJ-02. | Complete | `koduck-ai/src/adapters/strict_json.rs` created with the crate-private `ensure_unique_members` operation at `fd1ca905faf294885b0709f407911dd8b91f67e8`; the red-phase stub failed AC-1 with "malformed JSON was accepted", then passed after the visitor implementation. |
| T-2 | Migrate both consumers without changing their owned semantics. | `koduck-ai/src/adapters/tool.rs`; `koduck-ai/src/adapters/provider/stream_state.rs`; existing Tool and provider regressions for SJ-03/SJ-04. | Complete | Both consumers call `strict_json::ensure_unique_members` before semantic parsing at `fd1ca905faf294885b0709f407911dd8b91f67e8`; both private `UniqueJson` visitors were removed (net −185/+10 lines across the four touched files); AC-2 and AC-3 exit 0. |
| T-3 | Verify service-wide compatibility and governance evidence. | Routed Rust checks, contract traceability, risk matrix, source markers, and stable implementation evidence. | Complete | `cargo fmt --all --check`, `cargo clippy -p koduck-ai --all-targets --all-features -- -D warnings`, and `cargo test -p koduck-ai --all-targets --all-features` all exit 0 at `fd1ca905faf294885b0709f407911dd8b91f67e8`; governed-file markers added to all four touched files. |

**Affected paths**: `koduck-ai/src/adapters/mod.rs`,
`koduck-ai/src/adapters/strict_json.rs`, `koduck-ai/src/adapters/tool.rs`,
`koduck-ai/src/adapters/provider/stream_state.rs`, and the narrowest test files
needed for focused shared-validator coverage; this ADR and its central index
row receive lifecycle and evidence updates.

### Stable Implementation Touchpoints [Conditionally Required — source or configuration implementation]

| Path | Stable symbol or contract anchor | Key code excerpt, when needed | Purpose | Source revision |
| --- | --- | --- | --- | --- |
| `koduck-ai/src/adapters/strict_json.rs` | `koduck_ai::adapters::strict_json::ensure_unique_members` | N/A — the one crate-private operation and its `UniqueJson` visitor are the decisive implementation. | The single recursive duplicate-member rejection owner for both trust boundaries. | `fd1ca905faf294885b0709f407911dd8b91f67e8` |
| `koduck-ai/src/adapters/tool.rs` | `parse_action_parameters`; `parse_input_schema` | N/A — stable functions identify the decisive validation call sites, which now call `super::strict_json::ensure_unique_members`. | Applies size limits, strict syntax validation, semantic conversion, and Tool-owned error mapping. | `fd1ca905faf294885b0709f407911dd8b91f67e8` |
| `koduck-ai/src/adapters/provider/stream_state.rs` | `StreamState::parse_frame` | N/A — the stable method identifies the decisive lifecycle call site, which now calls `strict_json::ensure_unique_members`. | Rejects duplicated terminal evidence before `serde_json::Value` and maps it to provider code `INVALID_FRAME`. | `fd1ca905faf294885b0709f407911dd8b91f67e8` |
| `koduck-ai/tests/cand_2_policy.rs` | `duplicate_json_schema_members_fail_closed`; `duplicate_action_parameter_members_fail_closed_at_every_depth` | N/A — stable test symbols identify Tool contract evidence. | Proves exact Tool consumer mappings for nested duplicate members. | `fd1ca905faf294885b0709f407911dd8b91f67e8` |
| `koduck-ai/tests/openai_provider.rs` | `invalid_clean_end_sequences_fail_closed` | N/A — stable test symbol identifies provider contract evidence. | Proves duplicated provider terminal evidence fails as `INVALID_FRAME` without completion. | `fd1ca905faf294885b0709f407911dd8b91f67e8` |

**Migration and rollback strategy [Conditionally Required — this replaces or
changes existing behavior]**: The change replaces duplicated private
implementation but intentionally changes no accepted behavior or persisted
state. Implement the shared validator and focused tests first, then switch each
consumer while retaining its error conversion and existing regression. Stop if
any SJ clause or golden/contract test changes. Roll back by reverting the
source-only extraction to the two private visitors; no data migration,
compatibility window, or runtime recovery is required.

### Engineering Exceptions [Conditionally Required — an engineering rule is exceeded or waived]

N/A — the planned extraction reduces duplicated code and file size, introduces
no public surface, and does not exceed or waive an engineering rule.

## Contract-To-Check Traceability [Conditionally Required — source or configuration implementation]

| Clause ID | Authoritative contract path and heading | Exact normative requirement | Acceptance check or deterministic test IDs | Explicit coverage method |
| --- | --- | --- | --- | --- |
| SJ-01 | This ADR — Normative Contract Clauses; `koduck-ai/docs/contracts/cand-2-tool-approval-v1.md` — Native Tool And MCP Call Translation | Duplicate object members at every nesting depth are rejected before semantic parsing. | AC-1, AC-2, AC-3 | Shared-validator tables cover nested objects; Tool and provider consumer tests cover their production entry points. |
| SJ-02 | This ADR — Normative Contract Clauses | Valid duplicate-free JSON is accepted by the guard and malformed JSON is rejected. | AC-1 | Focused shared-validator tables exercise every JSON scalar family, arrays, objects, truncated input, invalid tokens, and duplicate names. |
| SJ-03 | This ADR — Normative Contract Clauses; `koduck-ai/docs/contracts/cand-2-tool-approval-v1.md` — Native Tool And MCP Call Translation | Tool action/schema duplicate inputs retain exact typed errors and pre-translation byte bounds. | AC-2, AC-4 | Existing exact-error regressions run with boundary-size and duplicate fixtures; routed suite detects mapping or bound changes. |
| SJ-04 | This ADR — Normative Contract Clauses; `docs/adr/ADR-0004-provider-stream-completion-normalization.md` — PSC-3/PSC-5 | Duplicate provider terminal evidence emits `INVALID_FRAME` and zero completion. | AC-3, AC-4 | The existing clean-end rejection table covers top-level and nested duplicate members and asserts the exact terminal event sequence. |
| SJ-05 | This ADR — Normative Contract Clauses; `docs/adr/ADR-0004-provider-stream-completion-normalization.md` — PSC-7 | No public surface, dependency, wire, state, error, or resource-limit behavior changes. | AC-2, AC-3, AC-4 | Consumer regressions plus full all-target/all-feature formatting, lint, test, and deterministic stable-symbol review cover compatibility and dependency direction. |

## Risk Coverage Matrix [Conditionally Required — source or configuration implementation]

| Risk dimension | Applicability and scenario, or specific N/A reason | Owning boundary | Deterministic verification method | Exact expected result | Acceptance check IDs | Status | Actual evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Concurrency and ordering | N/A — the validator owns no mutable or shared state and completes synchronously within one caller-owned parse. | `adapters::strict_json` | Inspect the validator signature and ownership model at its stable symbol; run strict Clippy. | The validator has no global/static mutable state, lock, task, thread, async operation, or observable ordering effect. | AC-1, AC-4 | Pass | `ensure_unique_members` is one synchronous pure parse with no static state at `fd1ca905faf294885b0709f407911dd8b91f67e8`; strict Clippy exits 0. |
| Timeout and deadline | N/A — the validator performs no I/O, wait, retry, or deadline operation; callers retain their existing transport and execution deadlines. | Tool and provider consumer boundaries | Run AC-2 through AC-4 and inspect the validator's stable operation. | No timeout constant, I/O operation, wait, retry, or caller deadline changes; existing suites pass. | AC-2, AC-3, AC-4 | Pass | The `fd1ca905faf294885b0709f407911dd8b91f67e8` diff introduces no timeout, I/O, wait, or retry code; AC-2 through AC-4 exit 0. |
| Cancellation and interruption | N/A — validation is one synchronous in-memory parse with no cancellation point, and the extraction does not change caller cancellation behavior. | Tool and provider consumer boundaries | Run AC-2 through AC-4 and inspect the validator's stable operation. | No cancellation/interruption type, transition, or consumer behavior changes; existing suites pass. | AC-2, AC-3, AC-4 | Pass | No cancellation type or transition changed at `fd1ca905faf294885b0709f407911dd8b91f67e8`; the cancellation and interruption suites in the full run exit 0. |
| Resource bounds and backpressure | Applicable — a shared recursive visitor could be invoked before a consumer-owned byte bound or allocate unexpectedly on attacker-controlled member counts. | Tool/provider bounds plus `adapters::strict_json` | AC-1 boundary fixtures, AC-2 Tool limit tests, AC-3 provider frame tests, and AC-4 full suite. | Existing Tool and provider byte limits and check ordering remain unchanged; all bounded-input regressions pass; no unbounded retained buffer or queue is introduced. | AC-1, AC-2, AC-3, AC-4 | Pass | The `fd1ca905faf294885b0709f407911dd8b91f67e8` diff swaps only the validator call after each existing size check; every bounded-input regression in the full suite passes; no new buffer or queue was introduced. |
| Framework or trust-boundary rejection | Applicable — duplicate or malformed untrusted JSON could be collapsed or mapped to the wrong consumer error after extraction. | Tool and OpenAI-compatible provider adapters | Run AC-1 through AC-3 with nested, malformed, duplicate-schema, duplicate-parameter, duplicate-top-level, and duplicate-finish fixtures. | Every fixture returns the exact SJ-01 through SJ-04 outcome and emits no accepted action/schema/completion from duplicated input. | AC-1, AC-2, AC-3 | Pass | AC-1 rejects malformed and duplicate fixtures at every declared depth; AC-2 returns exactly `InvalidJson`/`InvalidSchema` at both depths; AC-3 returns exactly `INVALID_FRAME` with zero `ProviderEvent::Completed`; all exit 0 at `fd1ca905faf294885b0709f407911dd8b91f67e8`. |

Allowed final statuses are `Pass`, `Fail`, or `N/A — <specific reason>`. `Fail`
blocks review-ready, `Complete`, and `Verified`.

## Acceptance Checks [Required]

| Check ID | Subtask | Binary acceptance point | Preconditions or input | Verification method | Exact expected result | Expected evidence | Status | Actual result and evidence |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| AC-1 | T-1 | The shared validator implements SJ-01 and SJ-02 for every JSON value family and nested object location. | Shared module exists; table contains `null`, booleans, signed/unsigned/fractional numbers, strings, arrays, nested objects, malformed input, top-level duplicates, and nested duplicates. | Run `cargo test -p koduck-ai --lib adapters::strict_json::tests::rejects_malformed_and_duplicate_json_at_every_depth -- --exact`. | Exit 0; every valid duplicate-free fixture returns `Ok`; every malformed or duplicate fixture returns `Err`; nested duplicates fail at every declared depth. | Command output and per-fixture assertions. | Pass | Exit 0; 1 passed, 272 filtered out at `fd1ca905faf294885b0709f407911dd8b91f67e8`; the red-phase stub was first observed failing on "malformed JSON was accepted". |
| AC-2 | T-2 | Tool action parameters and descriptor schemas retain SJ-03 error and bound behavior after migration. | Existing CAND-2 policy fixtures plus shared validator are wired through both Tool parsing entry points. | Run `cargo test -p koduck-ai --test cand_2_policy duplicate_`. | Exit 0; duplicate action parameters at both declared depths return exactly `ToolAdapterError::InvalidJson`; duplicate descriptor schema members return exactly `ToolAdapterError::InvalidSchema`; the owned duplicate-property constructor regression remains green. | Command output and exact error assertions. | Pass | Exit 0; 3 passed, 20 filtered out — `duplicate_action_parameter_members_fail_closed_at_every_depth`, `duplicate_json_schema_members_fail_closed`, and `owned_schema_constructor_rejects_duplicate_properties` all green at `fd1ca905faf294885b0709f407911dd8b91f67e8`. |
| AC-3 | T-2 | Provider duplicate members retain SJ-04 fail-closed terminal behavior after migration. | Existing clean-end table contains duplicate nested `finish_reason` and duplicate top-level `choices` cases. | Run `cargo test -p koduck-ai --test openai_provider invalid_clean_end_sequences_fail_closed -- --exact`. | Exit 0; both duplicate-member cases emit exactly provider code `INVALID_FRAME`, emit zero `ProviderEvent::Completed`, and the remainder of the declared invalid-sequence table remains green. | Command output and exact event/error assertions. | Pass | Exit 0; 1 passed, 16 filtered out at `fd1ca905faf294885b0709f407911dd8b91f67e8`; both duplicate-member cases and the full invalid-sequence table stayed green. |
| AC-4 | T-3 | The service compiles, formats, lints, and tests with one private strict-JSON owner and no public or dependency change. | T-1 and T-2 complete; Cargo manifests and accepted contracts are unchanged. | From the repository root run `cargo fmt --all --check`; `cargo clippy -p koduck-ai --all-targets --all-features -- -D warnings`; `cargo test -p koduck-ai --all-targets --all-features`; then inspect the compiler-resolved stable module and call-site symbols listed in this ADR. | Every command exits 0; one private `koduck_ai::adapters::strict_json` capability owns duplicate-member deserialization; both consumers call it and retain their error conversions; `Cargo.toml`, `Cargo.lock`, public exports, contract files, and wire fixtures have no diff. | Command outputs, diff, stable-symbol review, and tested commit SHA. | Pass | All three commands exit 0 at `fd1ca905faf294885b0709f407911dd8b91f67e8` (273 lib tests plus every integration suite); the change touches only `koduck-ai/src/adapters/{mod,strict_json,tool}.rs` and `koduck-ai/src/adapters/provider/stream_state.rs`; `Cargo.toml`, `Cargo.lock`, `koduck-ai/src/lib.rs`, contract files, and fixtures are unchanged; `mod strict_json;` is private and both consumers retain their exact error conversions. |

Allowed final check statuses are `Pass`, `Fail`, or `N/A — <specific reason>`.
`Fail` blocks completion. `N/A` is valid only when the check's stated trigger or
precondition demonstrably does not apply.

## Completion Checklist [Required]

| ID | Item | Completion Criterion | Expected Evidence | Status | Actual Evidence |
| --- | --- | --- | --- | --- | --- |
| A-1 | ADR approved | An eligible non-author approver, approval time, and exact `Approval Evidence: Approve` are recorded; any optional Approval Context Revision is informational, non-binding, and exactly represents the approved document | ADR metadata | Complete | @linhai approved at 2026-08-25T01:34:22Z with `Approval Evidence: Approve`; no Approval Context Revision is recorded because no immutable revision contained the document at approval time |
| A-2 | Complete task delivered | Every declared subtask has actual implementation evidence, every applicable acceptance check is `Pass` with actual result and evidence, and together they satisfy the complete task outcome | Implementation Plan and Acceptance Checks rows | Complete | T-1 through T-3 are `Complete` and AC-1 through AC-4 are `Pass` with command outputs pinned to `fd1ca905faf294885b0709f407911dd8b91f67e8`; one private strict-JSON owner serves both consumers with no consumer-visible change |
| A-3 | Reciprocal ADD link synchronized, when applicable | The selected candidate records this exact ADR path, this ADR records the exact ADD path and candidate ID, both references agree, and the candidate reaches `Complete` only with this ADR's `Complete` or `Verified` status | Exact ADD path, candidate ID, ADR path, and Git blob or commit | N/A — not product demand | N/A — this corrective internal maintenance task selects no ADD candidate |
| A-4 | Requirement levels satisfied | Every required section is complete, every conditional trigger is assessed and completed or marked `N/A — <reason>`, and optional sections are complete or removed | Structured document review | Complete | Structured review on 2026-08-25 found no blank required content or unassessed conditional trigger |
| A-5 | Acceptance checks are decidable | Every check names one subtask, preconditions or input, deterministic method, exact expected result, and evidence; no unqualified subjective criterion remains | Structured acceptance-check review | Complete | AC-1 through AC-4 name exact commands or deterministic stable-symbol inspection, exact expected results, and evidence |
| A-6 | Engineering exceptions governed, when applicable | Every exceeded or waived engineering rule has one complete exception row, an accountable owner, a lifecycle, and verification evidence before approval; otherwise the conditional subsection records `N/A — <reason>` | Engineering Exceptions subsection and affected-file evidence | N/A — no exception planned | The planned extraction does not exceed or waive an engineering rule |
| A-7 | Contract and baseline risks covered, when applicable | Every normative contract clause maps to an explicit check or deterministic test, and every required Risk Coverage Matrix row is complete before approval and reaches Pass or specific N/A before review-ready or completion | Contract-To-Check Traceability, Risk Coverage Matrix, acceptance checks, and stable evidence | Complete | SJ-01 through SJ-05 map to AC-1 through AC-4; all five baseline Risk Coverage Matrix rows are `Pass` with evidence pinned to `fd1ca905faf294885b0709f407911dd8b91f67e8` |
| A-8 | Governance validation passed | The independent validator reports no required-section, template-field, lifecycle-status, index, reciprocal-link, or Mermaid contract error for this record and repository | `npm run validate --prefix tools/governance-validator` output | Complete | Exit 0 on 2026-08-25 after the implementation evidence update; governance validation passed for the repository with this Accepted and Complete service ADR and central index row |

## Archival [Conditionally Required — Decision Status is `Rejected`, or Decision Status is `Deprecated` or `Superseded` and Implementation Status is final]

Archive this record in the same change that retires it when either Decision
Status is `Rejected` with Implementation Status `Not Applicable`, or Decision
Status is `Deprecated` or `Superseded` with Implementation Status `Verified`,
`Complete`, or `Not Applicable`. Before that trigger, retain this section as
inactive future-lifecycle guidance; its checklist does not affect acceptance or
implementation completion. When triggered:

- [ ] Move this file to `archive/ADR-0001-strict-json-duplicate-member-validation.md`
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
| 2026-08-25 | Implemented T-1 through T-3 at `fd1ca905faf294885b0709f407911dd8b91f67e8`: added the private `adapters::strict_json` validator with its characterization test, migrated both consumers off their private visitors, and recorded every acceptance check and risk row as `Pass`; Implementation Status set to `Complete`. Evidence-only update with no decision or scope change. | @zcode |
| 2026-08-25 | Accepted on @linhai's self-identified `Approve` response in the ADR-0001 review conversation; no approval context revision recorded because no immutable revision containing this document existed at approval time. | @zcode |
| 2026-08-25 | Aligned AC-4's Clippy and test commands exactly with the `koduck-ai/**` Scope Routing row after non-blocking review; no decision, scope, contract, or implementation status changed. | @codex |
| 2026-08-25 | Proposed one service-internal strict-JSON validation owner that removes duplicate visitors while preserving Tool and provider trust-boundary behavior. | @codex |
