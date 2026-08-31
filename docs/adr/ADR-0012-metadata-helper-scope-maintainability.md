# ADR-0012: Metadata Helper Scope Maintainability

## Metadata [Required]

- **Decision Status**: Accepted
- **Implementation Status**: In Progress
- **Date**: 2026-09-01
- **Author**: @codex
- **Decision Owner**: @linhai
- **Required Approver**: @linhai
- **Record Scope**: Project
- **Approver [Conditionally Required — Decision Status is or has been `Accepted`]**: @linhai
- **Approval Time [Conditionally Required — Decision Status is or has been `Accepted`]**: 2026-09-01T00:02:55+08:00
- **Approval Evidence [Conditionally Required — Decision Status is or has been `Accepted`]**: Approve
- **Rejector [Conditionally Required — Decision Status is `Rejected`]**: N/A — Decision Status is `Proposed`
- **Rejection Time [Conditionally Required — Decision Status is `Rejected`]**: N/A — Decision Status is `Proposed`
- **Rejection Evidence [Conditionally Required — Decision Status is `Rejected`]**: N/A — Decision Status is `Proposed`
- **Retired By [Conditionally Required — Decision Status is `Deprecated` or `Superseded`]**: N/A — record is not retired
- **Retirement Time [Conditionally Required — Decision Status is `Deprecated` or `Superseded`]**: N/A — record is not retired
- **Retirement Evidence [Conditionally Required — Decision Status is `Deprecated` or `Superseded`]**: N/A — record is not retired
- **Retirement Reason [Conditionally Required — Decision Status is `Deprecated` or `Superseded`]**: N/A — record is not retired
- **Blocked From [Conditionally Required — Implementation Status is `Blocked`]**: N/A — implementation is not blocked
- **Blocker And Evidence [Conditionally Required — Implementation Status is `Blocked`]**: N/A — implementation is not blocked
- **Blocker Owner [Conditionally Required — Implementation Status is `Blocked`]**: N/A — implementation is not blocked
- **Blocker Exit Or Recheck Criterion [Conditionally Required — Implementation Status is `Blocked`]**: N/A — implementation is not blocked
- **Related [Optional]**: Local SonarQube project `koduck` New Code issue observed 2026-09-01: Medium Maintainability issue at `tools/governance-validator/lib/metadata-validation.mjs:L10`, “Move function `fieldWithoutRequirementLevelSuffix` to the outer scope.”
- **Architecture Source [Conditionally Required — product demand]**: N/A — corrective governance-validator work requested from local SonarQube results, not derived from product demand
- **Supersedes [Conditionally Required — this ADR replaces another]**: N/A — no ADR is replaced
- **Superseded By [Conditionally Required — this ADR is replaced]**: None

## Requirement Level Legend [Required]

- **`[Required]`**: The section or field always applies and MUST remain present with complete, verifiable content. Use `None — <reason>` only when the template explicitly permits an empty result; never leave it blank.
- **`[Conditionally Required — <trigger>]`**: The section or field MUST be completed when its stated trigger applies. When the trigger does not apply, retain `N/A — <reason>` unless the template explicitly instructs removal or retention as inactive future-lifecycle guidance. A missing trigger assessment is incomplete content.
- **`[Optional]`**: The section may be removed without affecting acceptance, execution, completion, or verification. If retained, it MUST be accurate and complete; optional content MUST NOT substitute for required evidence.

Unlabeled fields inside a `[Required]` section are required.

## Context And Problem Statement [Required]

The processed local SonarQube version `c336192` reports one New Code Medium Maintainability issue at `tools/governance-validator/lib/metadata-validation.mjs:L10`: `fieldWithoutRequirementLevelSuffix` does not use `createMetadataValidator` closure state and should be module scoped. The function currently preserves the requirement-level suffix behavior completed under ADR-0011, so this correction must retain Metadata field normalization and duplicate-field diagnostics.

The local New Code issue is the red static-analysis baseline. Existing real-validator behavior already passes, so source-text assertions and artificial timing tests are prohibited. This task is deliberately separate from the twelve overall Reliability issues in `relationship-validation.mjs` and `validate.mjs`, which are independently reviewable module boundaries.

## Scope [Required]

In scope:

- Move `fieldWithoutRequirementLevelSuffix` from `createMetadataValidator` to module scope without changing its inputs, output, delimiter checks, or callers.
- Update the governed-file ADR marker to ADR-0012 and retain intent documentation for the helper and validator factory.
- Verify active Metadata duplicate-field behavior, the full governance-validator package suite, repository governance validation, and the scoped New Code analyzer result through a separately accepted OCR.

Out of scope:

- Changing Metadata field normalization, duplicate-field diagnostics, lifecycle rules, scanner configuration, issue workflow, credentials, or public contracts.
- Fixing the twelve overall Reliability issues in `relationship-validation.mjs` and `validate.mjs`.
- Refactoring other helpers, exporting the moved helper, or adding dependencies.

## Tensions, Constraints, And Open Questions [Required]

### Identified Tensions [Conditionally Required — competing goals or trade-offs exist]

| ID | Tension | Impact | Decision |
| --- | --- | --- | --- |
| TN-1 | The analyzer requests module scope, while the current nested helper visually groups all Metadata parsing behavior. | Moving the helper could accidentally change name resolution or field-normalization behavior; retaining it leaves the New Code quality-gate issue open. | Move only the closure-independent helper to module scope and keep the validator factory's callers and return contract unchanged. |

### Constraints [Required]

- This is a Full ADR because the project-wide governance validator enforces repository decision-record contracts.
- The source change must remain one independently reviewable implementation slice in `metadata-validation.mjs` and must not include the two remaining Reliability module boundaries.
- Existing real validator tests are behavioral evidence; the analyzer finding is the static red baseline and must not be tested with source-text or timing assertions.
- The first legal comment in the modified governed JavaScript file must cite this ADR.

### Open Questions [Conditionally Required — material questions exist or were resolved during drafting]

None — the helper has no closure dependencies and the local analyzer report identifies one scope-only correction.

## Decision Drivers [Required]

1. **Clear the New Code quality-gate issue**: The scoped analyzer result must have zero active rows for the helper-scope finding.
2. **Preserve Metadata behavior**: Canonical active-field extraction and duplicate-field diagnostics must remain unchanged.
3. **Maintain isolated ownership**: The correction must not enter the independently mergeable Reliability modules.

## Options Considered [Required]

### Option: Module-scope helper extraction

Move the closure-independent `fieldWithoutRequirementLevelSuffix` helper to module scope and retain its current call from `metadataEntry`.

Pros:

- Satisfies the analyzer's scope recommendation without changing helper behavior.
- Keeps the correction to one module and one primary implementation boundary.

Cons:

- Separates a helper from its current factory-local grouping.

### Option: Retain the nested helper

Leave the helper inside `createMetadataValidator`.

Pros:

- Requires no source movement.

Cons:

- Leaves the active New Code Maintainability issue and quality-gate failure unresolved.

### Option: Combine the remaining Reliability modules

Address all local SonarQube issues in one source change.

Pros:

- Could reduce the aggregate issue count sooner.

Cons:

- Mixes three independently reviewable primary module boundaries and violates the ADR scope rule.

## Decision [Required]

**Selected option**: Module-scope helper extraction.

**Rationale**: The helper's argument fully determines its result; moving it to the module boundary satisfies the analyzer without changing the established Metadata parsing contract. Retaining the existing factory and call site limits the change to the smallest coherent unit.

### Consequences [Required]

Positive:

- The New Code helper-scope finding is removed without changing external behavior.
- The code makes the helper's lack of closure dependency explicit.

Negative:

- The file contains one additional module-level non-exported symbol.
- Twelve unrelated Reliability findings remain for their own ADRs.

Mitigations:

- Preserve helper signature and `metadataEntry` call; run focused and complete deterministic checks; use a separately accepted local SonarQube OCR for analyzer confirmation.

## Implementation Plan [Required]

**Complete task outcome**: `fieldWithoutRequirementLevelSuffix` is module scoped, `metadataEntry` preserves active Metadata field normalization and duplicate detection, and separately authorized local SonarQube analysis has no active New Code helper-scope finding.

**Primary implementation boundary**: `tools/governance-validator/lib/metadata-validation.mjs` > `fieldWithoutRequirementLevelSuffix` module-scope placement.

Allowed subtask statuses: `Not Started`, `In Progress`, `Blocked`, `Complete`, or `N/A — <specific reason>`.

| ID | Objective or deliverable | Included scope | Status | Actual implementation evidence |
| --- | --- | --- | --- | --- |
| T-1 | Move the closure-independent suffix helper to module scope and update the governed-file marker. | `tools/governance-validator/lib/metadata-validation.mjs` > `fieldWithoutRequirementLevelSuffix`; file marker. | Complete | Commit `9ebf6a7` moves the unchanged helper to module scope and changes the governed-file marker to ADR-0012. |
| T-2 | Verify behavior, complete package contracts, and governance records. | Existing Node.js Metadata tests and `npm` scripts in `tools/governance-validator`. | In Progress | AC-1 through AC-3 pass against commit `9ebf6a7`; separately accepted OCR verification remains required for AC-4. |

**Affected paths**: `tools/governance-validator/lib/metadata-validation.mjs`; `tools/governance-validator/test/validation-boundary-regressions.test.mjs`; `docs/adr/ADR-0012-metadata-helper-scope-maintainability.md`; `docs/adr/INDEX.md`.

### Stable Implementation Touchpoints [Conditionally Required — source or configuration implementation]

| Path | Stable symbol or contract anchor | Key code excerpt, when needed | Purpose | Source revision |
| --- | --- | --- | --- | --- |
| `tools/governance-validator/lib/metadata-validation.mjs` | `createMetadataValidator` > `fieldWithoutRequirementLevelSuffix` | N/A — stable symbols are sufficient | Extracts a closure-independent field-suffix normalizer while retaining Metadata entry parsing. | `9ebf6a7` |
| `tools/governance-validator/test/validation-boundary-regressions.test.mjs` | `rejects duplicate active <field> metadata` | N/A — stable test anchor is sufficient | Exercises duplicate active Metadata entries through the real validator. | `c49bbe0` |

**Migration and rollback strategy [Conditionally Required — this replaces or changes existing behavior]**: Move only the existing helper declaration. Stop if focused duplicate-metadata behavior or the package suite fails. Rollback is a Git revert of the helper-extraction commit, restoring the nested declaration; no data or runtime migration is involved.

### Engineering Exceptions [Conditionally Required — an engineering rule is exceeded or waived]

N/A — the module-scope extraction follows the software-engineering standard without an exception.

## Contract-To-Check Traceability [Conditionally Required — source or configuration implementation]

| Clause ID | Authoritative contract path and heading | Exact normative requirement | Acceptance check or deterministic test IDs | Explicit coverage method |
| --- | --- | --- | --- | --- |
| TC-1 | `tools/governance-validator/lib/metadata-validation.mjs` — `metadataEntry` | Only active Metadata list entries with a bold field label and colon separator yield field/value records; a trailing requirement-level suffix is removed and field and value content are trimmed. | AC-1, AC-2 | The real validator runs duplicate-entry fixtures and the complete package suite. |
| TC-2 | `tools/governance-validator/lib/metadata-validation.mjs` — `validateUniqueMetadata` | Every repeated active canonical Metadata field produces the existing duplicate-field diagnostic. | AC-1, AC-2 | The focused duplicate-active-metadata tests assert status 1 and the canonical diagnostic for each active-field scenario. |

## Risk Coverage Matrix [Conditionally Required — source or configuration implementation]

| Risk dimension | Applicability and scenario, or specific N/A reason | Owning boundary | Deterministic verification method | Exact expected result | Acceptance check IDs | Status | Actual evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| concurrency and ordering | N/A — Metadata entry extraction processes one document synchronously and helper placement adds no state. | `createMetadataValidator` | Structured source review and AC-2. | No shared mutable state or ordering contract is introduced. | AC-2 | N/A — no concurrent behavior | Not run — terminal review follows implementation. |
| timeout and deadline | Applicable — a repository Metadata line may contain arbitrarily long requirement-level suffix text. | `fieldWithoutRequirementLevelSuffix` | AC-2 package suite and separately authorized analyzer reanalysis. | Package suite exits 0 and reanalysis has no active helper-scope finding. | AC-2, AC-4 | In Progress | AC-2 passed against commit `9ebf6a7`; OCR verification for AC-4 is not yet authorized. |
| cancellation and interruption | N/A — the validator CLI has no cancellation protocol and helper placement adds none. | Governance-validator CLI | Structured source review and AC-2. | No cancellation interface or lifecycle is introduced. | AC-2 | N/A — no cancellation protocol | Not run — terminal review follows implementation. |
| resource bounds and backpressure | Applicable — Metadata labels and suffixes remain repository-controlled input. | `fieldWithoutRequirementLevelSuffix` | AC-1 and AC-2. | Focused and complete real-validator checks exit 0 while preserving duplicate-entry behavior. | AC-1, AC-2 | Pass | AC-1 passed 3/3 selected tests and AC-2 passed 146/146 tests against commit `9ebf6a7`. |
| framework or trust-boundary rejection | Applicable — repository Markdown remains validator input that must retain canonical field/value extraction. | `createMetadataValidator` | AC-1 focused real-validator tests. | Focused tests exit 0 and retain existing duplicate-field diagnostics. | AC-1 | Pass | AC-1 passed 3/3 selected tests against commit `9ebf6a7`. |

## Acceptance Checks [Required]

| Check ID | Subtask | Binary acceptance point | Preconditions or input | Verification method | Exact expected result | Expected evidence | Status | Actual result and evidence |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| AC-1 | T-1 | The real validator continues to reject duplicate active Metadata fields with its canonical diagnostic. | Existing `rejects duplicate active <field> metadata` tests. | `node --test --test-name-pattern "rejects duplicate active" test/validation-boundary-regressions.test.mjs` in `tools/governance-validator`. | Process exits 0 with exactly three passing selected tests and no failed selected test. | Focused Node test report. | Pass | Passed 3/3 selected tests, 0 failures, after the helper extraction in commit `9ebf6a7`. |
| AC-2 | T-2 | The complete governance-validator suite preserves existing Metadata and governance-record contracts. | Approved implementation in the isolated task branch. | `npm test` in `tools/governance-validator`. | Process exits 0 with zero failed tests. | Full package test report. | Pass | Passed 146/146 tests, 0 failures, in 19.73 seconds after commit `9ebf6a7`. |
| AC-3 | T-2 | Repository governance validation accepts the ADR/index state and unchanged record contracts. | All task changes present in the isolated task branch. | `npm run validate` in `tools/governance-validator`. | Process exits 0 and reports `Governance validation passed.` | Governance-validation command output. | Pass | Passed after source commit `9ebf6a7`, reporting `Governance validation passed.` |
| AC-4 | T-2 | A separate accepted OCR verifies the analyzer outcome. | Approved source correction, AC-1 through AC-3 passed, and an accepted local SonarQube verification OCR. | The specified local New Code issue view after the OCR analysis completes. | Zero active New Code findings remain for `fieldWithoutRequirementLevelSuffix` helper scope. | OCR task and issue-view evidence without credentials. | Not Started | Not run — OCR may be drafted only after approved source work and deterministic checks. |

## Completion Checklist [Required]

| ID | Item | Completion Criterion | Expected Evidence | Status | Actual Evidence |
| --- | --- | --- | --- | --- |
| A-1 | ADR approved | An eligible non-author approver, approval time, and exact `Approval Evidence: Approve` are recorded. | ADR metadata | Complete | @linhai approved at 2026-09-01T00:02:55+08:00 with approval evidence `Approve`. |
| A-2 | Complete task delivered | T-1 through T-2 have implementation evidence and AC-1 through AC-4 are Pass. | Implementation Plan and Acceptance Checks | Not Started | Not run — awaiting implementation and OCR verification. |
| A-3 | Reciprocal ADD link synchronized, when applicable | N/A — the task is not derived from product demand and has no ADD candidate. | Metadata Architecture Source | N/A — no ADD applies | N/A — no product-demand ADD applies. |
| A-4 | Requirement levels satisfied | Every required section is complete, and every conditional trigger is completed or has a specific N/A reason. | Structured document review | Not Started | Not run — terminal review follows implementation. |
| A-5 | Acceptance checks are decidable | Every check has one subtask, input, deterministic method, exact expected result, and evidence. | Acceptance Checks table | Not Started | Not run — terminal review follows implementation. |
| A-6 | Engineering exceptions governed, when applicable | N/A — no engineering exception is planned. | Engineering Exceptions section | N/A — no exception applies | N/A — no engineering rule is exceeded or waived. |
| A-7 | Contract and baseline risks covered, when applicable | TC-1 through TC-2 map to checks, and every applicable risk reaches Pass before completion. | Traceability, matrix, and command reports | Not Started | Not run — awaiting implementation and analyzer verification. |
| A-8 | Governance validation passed | The independent validator reports no document or repository validation error. | `npm run validate` output | Not Started | Not run — draft validation follows document creation. |

## Supporting Notes [Optional]

The remaining two source slices are deliberately deferred: three Reliability issues in `relationship-validation.mjs`, then nine Reliability issues in `validate.mjs`. ADR serialization permits each only after this ADR reaches a truthful terminal implementation status.

## Archival [Conditionally Required — Decision Status is `Rejected`, or Decision Status is `Deprecated` or `Superseded` and Implementation Status is final]

The record is Accepted and not archival-eligible. If a later rejection, deprecation, or supersession triggers archival, move it under `docs/adr/archive/`, update all governed-file markers and references in the same change, and update its single index row.

## Change Log [Required]

| Date | Change | Author |
| --- | --- | --- |
| 2026-09-01 | Drafted the Full ADR for the local SonarQube New Code Metadata-helper scope finding. | @codex |
| 2026-09-01 | Accepted by @linhai with approval evidence `Approve` at 2026-09-01T00:02:55+08:00. | @codex |
