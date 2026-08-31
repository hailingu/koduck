# ADR-0009: Accepted-Record Validator Reliability

## Metadata [Required]

- **Decision Status**: Deprecated
- **Implementation Status**: Complete
- **Date**: 2026-08-31
- **Author**: @codex
- **Decision Owner**: @linhai
- **Required Approver**: @linhai
- **Record Scope**: Project
- **Approver [Conditionally Required — Decision Status is or has been `Accepted`]**: @linhai
- **Approval Time [Conditionally Required — Decision Status is or has been `Accepted`]**: 2026-08-31T21:33:34+08:00
- **Approval Evidence [Conditionally Required — Decision Status is or has been `Accepted`]**: Approve
- **Rejector [Conditionally Required — Decision Status is `Rejected`]**: N/A — Decision Status is `Proposed`
- **Rejection Time [Conditionally Required — Decision Status is `Rejected`]**: N/A — Decision Status is `Proposed`
- **Rejection Evidence [Conditionally Required — Decision Status is `Rejected`]**: N/A — Decision Status is `Proposed`
- **Retired By [Conditionally Required — Decision Status is `Deprecated` or `Superseded`]**: @linhai
- **Retirement Time [Conditionally Required — Decision Status is `Deprecated` or `Superseded`]**: 2026-08-31T22:01:15+08:00
- **Retirement Evidence [Conditionally Required — Decision Status is `Deprecated` or `Superseded`]**: Deprecate
- **Retirement Reason [Conditionally Required — Decision Status is `Deprecated` or `Superseded`]**: The completed accepted-record remediation is retained as historical evidence; the Decision Owner directed archival before further Reliability remediation.
- **Blocked From [Conditionally Required — Implementation Status is `Blocked`]**: N/A — implementation is not blocked
- **Blocker And Evidence [Conditionally Required — Implementation Status is `Blocked`]**: N/A — implementation is not blocked
- **Blocker Owner [Conditionally Required — Implementation Status is `Blocked`]**: N/A — implementation is not blocked
- **Blocker Exit Or Recheck Criterion [Conditionally Required — Implementation Status is `Blocked`]**: N/A — implementation is not blocked
- **Related [Optional]**: Local SonarQube overall Reliability issue list for project `koduck`, observed 2026-08-31; following archived `docs/adr/archive/ADR-0008-delimiter-bounded-governance-record-paths.md`.
- **Architecture Source [Conditionally Required — product demand]**: N/A — corrective governance-validator work requested from the local SonarQube result, not derived from product demand
- **Supersedes [Conditionally Required — this ADR replaces another]**: N/A — no ADR is replaced
- **Superseded By [Conditionally Required — this ADR is replaced]**: None

## Requirement Level Legend [Required]

- **`[Required]`**: The section or field always applies and MUST remain present with complete, verifiable content. Use `None — <reason>` only when the template explicitly permits an empty result; never leave it blank.
- **`[Conditionally Required — <trigger>]`**: The section or field MUST be completed when its stated trigger applies. When the trigger does not apply, retain `N/A — <reason>` unless the template explicitly instructs removal or retention as inactive future-lifecycle guidance. A missing trigger assessment is incomplete content.
- **`[Optional]`**: The section may be removed without affecting acceptance, execution, completion, or verification. If retained, it MUST be accurate and complete; optional content MUST NOT substitute for required evidence.

Unlabeled fields inside a `[Required]` section are required.

## Context And Problem Statement [Required]

The overall local SonarQube Reliability view reports four open findings in `tools/governance-validator/lib/accepted-records.mjs`: one High finding for passing `isCompleteTableValue` directly to `Array#every`, two Medium findings for a repeated-whitespace regular expression and a conditional that returns the same value on both branches, and one Low finding for use of global `String#replace` instead of `String#replaceAll`.

These sites all belong to `createAcceptedRecordValidator`, which validates accepted ADR, OCR, and ADD content. The reported issues are static-analysis findings: the existing test suite already exercises the affected validation paths, so the observed SonarQube finding set is the red baseline while the established behavior-preservation tests guard the refactor.

## Scope [Required]

In scope:

- Replace the direct predicate callback with an explicit single-value callback at the Stable Implementation Touchpoints check.
- Reuse the existing `isReasonedNa` contract after leading-whitespace normalization instead of duplicating its expression.
- Collapse the redundant legacy-status fallback and use literal global replacement for record-status normalization.
- Preserve the existing accepted-record validation outcomes through the focused and complete package test suites, then verify the four finding locations through a separately authorized local reanalysis.

Out of scope:

- Changing the accepted-record schema, lifecycle statuses, validation diagnostics, command-line interface, SonarQube configuration, profiles, issue workflow, or credentials.
- Fixing the remaining 15 overall Reliability findings in `mermaid-validation.mjs`, `metadata-validation.mjs`, `relationship-validation.mjs`, or `validate.mjs`.
- Submitting a new SonarQube analysis; that local operation requires a separate accepted OCR after source verification.

## Tensions, Constraints, And Open Questions [Required]

### Identified Tensions [Conditionally Required — competing goals or trade-offs exist]

| ID | Tension | Impact | Decision |
| --- | --- | --- | --- |
| TN-1 | The four analyzer findings should be removed without changing acceptance of existing governance records. | A broad parser rewrite could alter lifecycle validation; leaving the expressions and ambiguous callback retains the reported findings. | Apply only local, behavior-preserving simplifications within `createAcceptedRecordValidator` and retain the existing validation suite as the semantic guard. |

### Constraints [Required]

- This is a Full ADR because the validator enforces repository-wide governance-record contracts and consumes repository-controlled input.
- No dependency, public contract, record schema, scanner configuration, or issue state change is permitted.
- The SonarQube finding set is the static-analysis red baseline; no source-text assertion or artificial test may be introduced for a behavior that is already correct.
- Every modified governed JavaScript source file must cite this ADR at its first legal comment position.

### Open Questions [Conditionally Required — material questions exist or were resolved during drafting]

None — the source locations, existing validator contracts, and behavior-preservation checks identify the complete first implementation slice.

## Decision Drivers [Required]

1. **Remove the four reported findings**: the analyzer must report no active finding at the four accepted-record locations after authorized reanalysis.
2. **Preserve validation semantics**: accepted-record outcomes for required table cells, reasoned N/A values, lifecycle statuses, and normalized risk dimensions remain unchanged.
3. **Keep ownership narrow**: the change remains inside one accepted-record validator boundary without touching the remaining Reliability work.

## Options Considered [Required]

### Option: Local behavior-preserving simplifications

Use an explicit single-value callback, the existing reasoned-N/A helper after `trimStart`, a direct nullish fallback, and literal global replacement.

Pros:

- Removes all four analyzer-reported patterns in one cohesive validator boundary.
- Reuses existing validation semantics rather than adding a parser or dependency.

Cons:

- Requires a focused review to confirm each replacement preserves the current contract.

### Option: Suppress or relabel the findings

Leave the source unchanged and modify analyzer state or configuration.

Pros:

- Avoids a source change.

Cons:

- Retains the ambiguous callback and duplicated expression.
- Changes analyzer workflow outside this implementation boundary.

### Option: Combine every remaining Reliability issue in one change

Modify all reported files under the governance validator together.

Pros:

- Could reduce the total issue count in one iteration.

Cons:

- Mixes independently reviewable validator boundaries and violates the one-slice ADR scope rule.

## Decision [Required]

**Selected option**: Local behavior-preserving simplifications.

**Rationale**: The four sites have one owner and rely on already-defined validation rules. Explicitly expressing those rules through existing helpers and literal operations removes the analyzer concerns while the real validator suite exercises observable record-validation outcomes.

### Consequences [Required]

Positive:

- The High, Medium, and Low findings in `accepted-records.mjs` no longer depend on ambiguous callback, duplicated expression, redundant branch, or regular-expression replacement forms.
- Existing accepted-record semantic tests remain the authority for behavior.

Negative:

- The remaining 15 Reliability findings require later, separately scoped decisions.

Mitigations:

- Keep all other files out of scope, run the focused and full validator suites, and verify this slice through a separately accepted OCR.

## Implementation Plan [Required]

**Complete task outcome**: `createAcceptedRecordValidator` removes the four reported Reliability patterns while preserving the existing accepted-record validation outcomes; after separately authorized reanalysis, no active SonarQube Reliability finding remains at those four locations.

**Primary implementation boundary**: `tools/governance-validator/lib/accepted-records.mjs` > `createAcceptedRecordValidator` accepted-stage governance-record validation.

Allowed subtask statuses: `Not Started`, `In Progress`, `Blocked`, `Complete`, or `N/A — <specific reason>`.

| ID | Objective or deliverable | Included scope | Status | Actual implementation evidence |
| --- | --- | --- | --- | --- |
| T-1 | Replace the four local analyzer-reported patterns with behavior-preserving operations and update the governed-file marker. | `createAcceptedRecordValidator`, `isReasonedNa`, `validateStableImplementationTouchpoints`, `validateRiskMatrixDimensions`, and `validateAcceptedRiskMatrix` in `tools/governance-validator/lib/accepted-records.mjs`. | Complete | Commit `81a7c51` uses an explicit single-value callback, reuses `isReasonedNa` after `trimStart`, removes the redundant legacy-status branch, uses `replaceAll` for literal hyphens, and updates the file marker to this ADR. |
| T-2 | Verify accepted-record behavior, the complete validator package, and governance documentation contracts. | Existing Node.js tests and `npm` scripts in `tools/governance-validator`. | Complete | The focused real-validator test passed in about 280 ms; `npm test` passed 146/146 tests; `npm run validate` passed. OCR verification remains required for AC-4. |

**Affected paths**: `tools/governance-validator/lib/accepted-records.mjs`; `tools/governance-validator/test/validate-structure.test.mjs`; `docs/adr/archive/ADR-0009-accepted-record-validator-reliability.md`; `docs/adr/INDEX.md`.

### Stable Implementation Touchpoints [Conditionally Required — source or configuration implementation]

| Path | Stable symbol or contract anchor | Key code excerpt, when needed | Purpose | Source revision |
| --- | --- | --- | --- | --- |
| `tools/governance-validator/lib/accepted-records.mjs` | `createAcceptedRecordValidator` > `validateStableImplementationTouchpoints` | N/A — stable symbol is sufficient | Checks required Stable Implementation Touchpoints cells without allowing array callback metadata to become predicate input. | `4be9383` |
| `tools/governance-validator/lib/accepted-records.mjs` | `isReasonedNa` and `validateRiskMatrixDimensions` | N/A — stable symbols are sufficient | Preserves the sole reasoned-N/A form after equivalent leading-whitespace normalization. | `4be9383` |
| `tools/governance-validator/lib/accepted-records.mjs` | `validateAcceptedRiskMatrix` > legacy status normalization and `normalizeDimension` | N/A — stable symbols are sufficient | Preserves status fallback and global hyphen/whitespace normalization without redundant conditional or regular-expression replacement. | `4be9383` |
| `tools/governance-validator/test/validate-structure.test.mjs` | `accepts an explicit N/A Risk Coverage Matrix with a reason` and full test suite | N/A — stable test anchors are sufficient | Exercises the actual validator with a reasoned-N/A risk-matrix fixture and guards accepted-record behavior. | `4be9383` |

**Migration and rollback strategy [Conditionally Required — this replaces or changes existing behavior]**: Replace only local equivalent operations; stop if a focused accepted-record test or complete package test fails. Rollback is a Git revert of the implementation commit, restoring the prior expressions and callback form; no data or runtime migration is involved.

### Engineering Exceptions [Conditionally Required — an engineering rule is exceeded or waived]

N/A — the localized replacements do not exceed or waive a software-engineering rule.

## Contract-To-Check Traceability [Conditionally Required — source or configuration implementation]

| Clause ID | Authoritative contract path and heading | Exact normative requirement | Acceptance check or deterministic test IDs | Explicit coverage method |
| --- | --- | --- | --- | --- |
| TC-1 | `tools/governance-validator/lib/accepted-records.mjs` — `validateStableImplementationTouchpoints` | Required path, purpose, and revision table cells must be complete; anchors and excerpts must be complete or reasoned N/A. | AC-1, AC-2 | The real validator runs accepted-record fixtures through focused and complete Node test suites. |
| TC-2 | `tools/governance-validator/lib/accepted-records.mjs` — `isReasonedNa` and `validateRiskMatrixDimensions` | A reasoned N/A requires the `N/A — <reason>` form; leading whitespace in a matrix-level value does not change recognition. | AC-1, AC-2 | The focused real-validator fixture accepts the required reasoned-N/A form and the package suite preserves adjacent rejection behavior. |
| TC-3 | `tools/governance-validator/lib/accepted-records.mjs` — `validateAcceptedRiskMatrix` and `normalizeDimension` | Legacy status fallback returns the original status when no recognized status is extracted; normalization removes all hyphens and collapses whitespace. | AC-2 | The complete real-validator suite preserves existing lifecycle and risk-matrix validation results. |

## Risk Coverage Matrix [Conditionally Required — source or configuration implementation]

| Risk dimension | Applicability and scenario, or specific N/A reason | Owning boundary | Deterministic verification method | Exact expected result | Acceptance check IDs | Status | Actual evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| concurrency and ordering | N/A — the validator processes one input synchronously and this change adds no shared state. | `createAcceptedRecordValidator` | Structured source review and AC-2. | No shared mutable state or ordering contract is introduced. | AC-2 | N/A — no concurrent behavior | Source review confirms the local replacements introduce no shared state or ordering behavior. |
| timeout and deadline | Applicable — the repeated-whitespace expression accepts untrusted record text. | `validateRiskMatrixDimensions` | AC-2 package test suite and separately authorized analyzer reanalysis. | The package test suite exits 0, and reanalysis has no active finding at the reported expression location. | AC-2, AC-4 | Pass | `npm test` passed 146/146 tests, and accepted OCR-0005 verified no active accepted-record Reliability row after the `81a7c51` analysis. |
| cancellation and interruption | N/A — the CLI has no cancellation protocol and the local replacements add none. | Governance-validator CLI | Structured source review and AC-2. | No cancellation interface or lifecycle is introduced. | AC-2 | N/A — no cancellation protocol | Source review confirms the local replacements add no cancellation interface or lifecycle. |
| resource bounds and backpressure | Applicable — record text may contain arbitrary leading whitespace and dimension values. | `isReasonedNa` and `normalizeDimension` | AC-1 and AC-2. | Existing focused fixture and complete package suite exit 0 without changing accepted/rejected outcomes. | AC-1, AC-2 | Pass | The focused test and full package suite passed after commit `81a7c51`, preserving the reasoned-N/A validation path. |
| framework or trust-boundary rejection | Applicable — repository Markdown is validator input that must retain complete-cell and reasoned-N/A rejection behavior. | `createAcceptedRecordValidator` | AC-1 focused real-validator test. | The focused test exits 0, preserving the fixture’s accepted reasoned-N/A result and existing rejection diagnostics. | AC-1 | Pass | The focused real-validator fixture passed after the local replacements, preserving its accepted reasoned-N/A result. |

## Acceptance Checks [Required]

| Check ID | Subtask | Binary acceptance point | Preconditions or input | Verification method | Exact expected result | Expected evidence | Status | Actual result and evidence |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| AC-1 | T-1 | The real validator retains accepted reasoned-N/A risk-matrix behavior after the local replacements. | The existing `accepts an explicit N/A Risk Coverage Matrix with a reason` fixture. | `node --test --test-name-pattern "accepts an explicit N/A Risk Coverage Matrix with a reason" test/validate-structure.test.mjs` in `tools/governance-validator`. | Process exits 0 with exactly one passing selected test and no failed selected test. | Focused Node test report. | Pass | The selected real-validator test passed after the change in about 280 ms with one passing test and zero failures. |
| AC-2 | T-2 | The complete governance-validator suite preserves existing accepted-record contracts. | Accepted implementation in the isolated task branch. | `npm test` in `tools/governance-validator`. | Process exits 0 with zero failed tests. | Full package test report. | Pass | `npm test` exited 0 with 146/146 tests passing after commit `81a7c51`. |
| AC-3 | T-2 | Repository governance validation accepts the ADR/index state and unchanged record contracts. | All task changes present in the isolated task branch. | `npm run validate` in `tools/governance-validator`. | Process exits 0 and reports `Governance validation passed.` | Governance-validation command output. | Pass | `npm run validate` exited 0 and reported `Governance validation passed.` after the source correction. |
| AC-4 | T-2 | A separate accepted OCR verifies the first-slice analyzer outcome. | Source correction, AC-1 through AC-3 passed, and accepted OCR-0005. | The specified local Reliability issue view after the OCR’s analysis completes. | Exactly zero active Reliability findings remain at the four former `accepted-records.mjs` locations. | OCR task and issue-view evidence without credentials. | Pass | Archived OCR-0005 records completed version `81a7c51`, Quality Gate `Passed`, and no active `accepted-records.mjs` row in the Open/Confirmed overall Reliability view. |

## Completion Checklist [Required]

| ID | Item | Completion Criterion | Expected Evidence | Status | Actual Evidence |
| --- | --- | --- | --- | --- | --- |
| A-1 | ADR approved | An eligible non-author approver, approval time, and exact `Approval Evidence: Approve` are recorded. | ADR metadata | Complete | @linhai approved at 2026-08-31T21:33:34+08:00 with `Approval Evidence: Approve`. |
| A-2 | Complete task delivered | T-1 through T-2 have implementation evidence and AC-1 through AC-4 are Pass. | Implementation Plan and Acceptance Checks | Complete | T-1 and T-2 have implementation evidence; AC-1 through AC-3 passed locally and AC-4 passed through archived OCR-0005. |
| A-3 | Reciprocal ADD link synchronized, when applicable | N/A — the task is not derived from product demand and has no ADD candidate. | Metadata Architecture Source | N/A — no ADD applies | N/A — no product-demand ADD applies. |
| A-4 | Requirement levels satisfied | Every required section is complete, and every conditional trigger is completed or has a specific N/A reason. | Structured document review | Complete | Terminal document review completed; the independent governance validator accepted every ADR, OCR, and index contract. |
| A-5 | Acceptance checks are decidable | Every check has one subtask, input, deterministic method, exact expected result, and evidence. | Acceptance Checks table | Complete | AC-1 through AC-4 each name a subtask, deterministic verification, binary outcome, and captured evidence. |
| A-6 | Engineering exceptions governed, when applicable | N/A — no engineering exception is planned. | Engineering Exceptions section | N/A — no exception applies | N/A — no engineering rule is exceeded or waived. |
| A-7 | Contract and baseline risks covered, when applicable | TC-1 through TC-3 map to checks, and every applicable risk reaches Pass before completion. | Traceability, matrix, and command reports | Complete | TC-1 through TC-3 map to AC-1 through AC-2; all applicable risk rows now pass, including analyzer verification through OCR-0005. |
| A-8 | Governance validation passed | The independent validator reports no document or repository validation error. | `npm run validate` output | Complete | `npm run validate` exited 0 after the source correction and again for this terminal evidence revision. |

## Supporting Notes [Optional]

The remaining 15 overall Reliability findings are explicitly deferred so that this first slice remains independently reviewable. The active overall count can remain nonzero after this slice; AC-4 concerns only the four accepted-record locations.

## Archival [Conditionally Required — Decision Status is `Rejected`, or Decision Status is `Deprecated` or `Superseded` and Implementation Status is final]

The record is `Deprecated / Complete` and is archived under `docs/adr/archive/`; all governed-file markers, references, and its index row use the archived path.

## Change Log [Required]

| Date | Change | Author |
| --- | --- | --- |
| 2026-08-31 | Drafted the Full ADR for the first four overall SonarQube Reliability findings in the accepted-record validator. | @codex |
| 2026-08-31 | Accepted by @linhai with approval evidence `Approve` at 2026-08-31T21:33:34+08:00. | @codex |
| 2026-08-31 | Implemented local accepted-record simplifications in commit `81a7c51`; focused test, full package suite, and governance validation passed. | @codex |
| 2026-08-31 | Archived OCR-0005 after local analysis version `81a7c51` passed its Quality Gate and the overall Reliability view showed no active accepted-record target row. | @codex |
| 2026-08-31 | @linhai issued `Deprecate` at 2026-08-31T22:01:15+08:00; archived the completed accepted-record remediation decision with no replacement record. | @codex |
