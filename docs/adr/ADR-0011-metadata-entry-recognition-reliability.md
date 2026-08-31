# ADR-0011: Metadata Entry Recognition Reliability

## Metadata [Required]

- **Decision Status**: Proposed
- **Implementation Status**: Not Started
- **Date**: 2026-08-31
- **Author**: @codex
- **Decision Owner**: @linhai
- **Required Approver**: @linhai
- **Record Scope**: Project
- **Approver [Conditionally Required — Decision Status is or has been `Accepted`]**: N/A — Decision Status is `Proposed`
- **Approval Time [Conditionally Required — Decision Status is or has been `Accepted`]**: N/A — Decision Status is `Proposed`
- **Approval Evidence [Conditionally Required — Decision Status is or has been `Accepted`]**: N/A — Decision Status is `Proposed`
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
- **Related [Optional]**: Local SonarQube overall Reliability issue list for project `koduck`, observed 2026-08-31 after version `4d9c274`.
- **Architecture Source [Conditionally Required — product demand]**: N/A — corrective governance-validator work requested from the local SonarQube result, not derived from product demand
- **Supersedes [Conditionally Required — this ADR replaces another]**: N/A — no ADR is replaced
- **Superseded By [Conditionally Required — this ADR is replaced]**: None

## Requirement Level Legend [Required]

- **`[Required]`**: The section or field always applies and MUST remain present with complete, verifiable content. Use `None — <reason>` only when the template explicitly permits an empty result; never leave it blank.
- **`[Conditionally Required — <trigger>]`**: The section or field MUST be completed when its stated trigger applies. When the trigger does not apply, retain `N/A — <reason>` unless the template explicitly instructs removal or retention as inactive future-lifecycle guidance. A missing trigger assessment is incomplete content.
- **`[Optional]`**: The section may be removed without affecting acceptance, execution, completion, or verification. If retained, it MUST be accurate and complete; optional content MUST NOT substitute for required evidence.

Unlabeled fields inside a `[Required]` section are required.

## Context And Problem Statement [Required]

The local SonarQube overall Reliability view reports two Open Medium findings at `tools/governance-validator/lib/metadata-validation.mjs` lines 12 and 13. The Metadata-section entry recognizer uses one unbounded lazy capture for the bold field label and one trailing wildcard capture for the value; the analyzer identifies both as potentially super-linear on repository Markdown lines.

`entries` supplies field/value records to every metadata uniqueness and lifecycle check. The established `rejects duplicate active <field> metadata` test family exercises its observable contract for real Metadata fields. The analyzer findings are the red baseline: that existing behavior already passes before the correction, so source-text or artificial timing assertions would be brittle and are prohibited.

## Scope [Required]

In scope:

- Replace only the reported Metadata-entry recognition expression in `entries` with bounded delimiter-aware parsing that preserves valid active Metadata entry extraction.
- Preserve the current behavior that strips requirement-level suffixes from field labels, trims field and value content, and ignores non-entry lines outside the real Metadata section.
- Verify existing duplicate-active-metadata behavior, the package suite, and governance validation; verify the two target analyzer findings through a separately accepted local SonarQube OCR.

Out of scope:

- Changing lifecycle rules, duplicate-field diagnostics, required-field semantics, metadata schema, scanner configuration, issue workflow, credentials, or the twelve other Reliability findings in `relationship-validation.mjs` and `validate.mjs`.
- Changing `mermaid-validation.mjs`, whose preceding reliability correction is complete under ADR-0010.
- Submitting a SonarQube analysis; that operational step requires a separate accepted OCR after source verification.

## Tensions, Constraints, And Open Questions [Required]

### Identified Tensions [Conditionally Required — competing goals or trade-offs exist]

| ID | Tension | Impact | Decision |
| --- | --- | --- | --- |
| TN-1 | Metadata lines require structural parsing, but a broad parser rewrite could alter field normalization or accept non-metadata prose. | Changing field/value boundaries can invalidate lifecycle and duplicate-field checks; retaining the expression leaves two analyzer findings. | Replace only line-entry recognition with explicit prefix, delimiter, and suffix checks; retain section selection and normalization. |

### Constraints [Required]

- This is a Full ADR because the validator enforces repository-wide governance-record contracts and consumes repository-controlled Markdown.
- No dependency, public contract, record schema, scanner configuration, issue state, or credential handling change is permitted.
- The analyzer findings are the red static-analysis baseline; no source-text assertion, artificial timing threshold, or test that already passes before correction may be presented as a newly failing behavioral regression.
- The modified governed JavaScript file must cite this ADR at its first legal comment position.

### Open Questions [Conditionally Required — material questions exist or were resolved during drafting]

None — the two findings share one entry-recognition expression and one primary implementation boundary.

## Decision Drivers [Required]

1. **Remove both reported findings**: authorized reanalysis must have zero active Reliability findings at the former Metadata-entry expression locations.
2. **Preserve active Metadata semantics**: duplicate active fields remain detected with their canonical field names and trimmed values.
3. **Keep ownership narrow**: no other Reliability owner enters this decision.

## Options Considered [Required]

### Option: Delimiter-aware Metadata-entry parsing

Recognize the exact list-item and bold-label delimiters, locate the first closing bold delimiter, then read and trim the remaining value without an unbounded whole-line regular expression.

Pros:

- Removes both analyzer-reported captures while making field boundaries explicit.
- Keeps `entries` local and preserves its returned record shape.

Cons:

- Requires precise preservation of the existing Markdown entry grammar.

### Option: Suppress or relabel the findings

Leave the source expression unchanged and alter analyzer state or configuration.

Pros:

- Avoids a source edit.

Cons:

- Retains the analyzer-reported unbounded form.
- Changes analyzer workflow outside the implementation boundary.

### Option: Combine the remaining validator modules

Modify every outstanding Reliability owner in one change.

Pros:

- Could reduce the aggregate count more quickly.

Cons:

- Mixes independently reviewable primary implementation boundaries and violates ADR scope rules.

## Decision [Required]

**Selected option**: Delimiter-aware Metadata-entry parsing.

**Rationale**: `entries` already owns Metadata list-item recognition. A local delimiter-aware parser removes both reported forms while the established real validator tests preserve the observable uniqueness contract.

### Consequences [Required]

Positive:

- The Metadata entry recognizer no longer uses the two analyzer-reported potentially super-linear captures.
- Existing active-field normalization and duplicate detection remain guarded by real validator tests.

Negative:

- Twelve overall Reliability findings remain for separately scoped decisions.

Mitigations:

- Touch only `entries`, run focused and full deterministic checks, and use a separately accepted OCR for analyzer verification.

## Implementation Plan [Required]

**Complete task outcome**: `entries` replaces the reported Metadata-entry expression with bounded delimiter-aware parsing while preserving active Metadata field/value extraction and duplicate detection; after separately authorized reanalysis, no active Reliability finding remains at either former target location.

**Primary implementation boundary**: `tools/governance-validator/lib/metadata-validation.mjs` > `createMetadataValidator` > `entries` Metadata-list-item recognition.

Allowed subtask statuses: `Not Started`, `In Progress`, `Blocked`, `Complete`, or `N/A — <specific reason>`.

| ID | Objective or deliverable | Included scope | Status | Actual implementation evidence |
| --- | --- | --- | --- | --- |
| T-1 | Replace the reported Metadata-entry expression with bounded delimiter-aware parsing and update the governed-file marker. | `createMetadataValidator` > `entries` in `tools/governance-validator/lib/metadata-validation.mjs`. | Not Started | Not run — awaiting approval. |
| T-2 | Verify active Metadata duplicate detection, the complete package suite, and governance documentation contracts. | Existing Node.js tests and `npm` scripts in `tools/governance-validator`. | Not Started | Not run — awaiting T-1 implementation. |

**Affected paths**: `tools/governance-validator/lib/metadata-validation.mjs`; `tools/governance-validator/test/validation-boundary-regressions.test.mjs`; `docs/adr/ADR-0011-metadata-entry-recognition-reliability.md`; `docs/adr/INDEX.md`.

### Stable Implementation Touchpoints [Conditionally Required — source or configuration implementation]

| Path | Stable symbol or contract anchor | Key code excerpt, when needed | Purpose | Source revision |
| --- | --- | --- | --- | --- |
| `tools/governance-validator/lib/metadata-validation.mjs` | `createMetadataValidator` > `entries` | N/A — stable symbol is sufficient | Extracts active Metadata field/value records for duplicate and lifecycle validation. | `89f51c3` |
| `tools/governance-validator/test/validation-boundary-regressions.test.mjs` | `rejects duplicate active <field> metadata` | N/A — stable test anchor is sufficient | Exercises duplicate active Metadata entries through the real validator. | `89f51c3` |

**Migration and rollback strategy [Conditionally Required — this replaces or changes existing behavior]**: Replace only Metadata entry recognition; stop if focused duplicate-metadata behavior or the complete package suite fails. Rollback is a Git revert of the implementation commit, restoring the prior expression; no data or runtime migration is involved.

### Engineering Exceptions [Conditionally Required — an engineering rule is exceeded or waived]

N/A — the localized replacement does not exceed or waive a software-engineering rule.

## Contract-To-Check Traceability [Conditionally Required — source or configuration implementation]

| Clause ID | Authoritative contract path and heading | Exact normative requirement | Acceptance check or deterministic test IDs | Explicit coverage method |
| --- | --- | --- | --- | --- |
| TC-1 | `tools/governance-validator/lib/metadata-validation.mjs` — `entries` | Only active Metadata list entries with a bold field label and colon separator yield field/value records; field requirement-level suffixes are removed and both values are trimmed. | AC-1, AC-2 | The real validator runs duplicate-entry fixtures and the complete package suite. |
| TC-2 | `tools/governance-validator/lib/metadata-validation.mjs` — `validateUniqueMetadata` | Every repeated active canonical Metadata field produces the existing duplicate-field diagnostic. | AC-1, AC-2 | The focused duplicate-active-metadata tests assert status 1 and the canonical diagnostic for each active-field scenario. |

## Risk Coverage Matrix [Conditionally Required — source or configuration implementation]

| Risk dimension | Applicability and scenario, or specific N/A reason | Owning boundary | Deterministic verification method | Exact expected result | Acceptance check IDs | Status | Actual evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| concurrency and ordering | N/A — entry extraction processes one document synchronously and the replacement adds no shared state. | `entries` | Structured source review and AC-2. | No shared mutable state or ordering contract is introduced. | AC-2 | N/A — no concurrent behavior | Not run — implementation not started. |
| timeout and deadline | Applicable — a repository Metadata line may contain arbitrarily long label or value text. | `entries` | AC-2 package suite and separately authorized analyzer reanalysis. | Package suite exits 0, and reanalysis has no active finding at either reported Metadata-entry location. | AC-2, AC-4 | Not Started | SonarQube reports two Open Medium Reliability findings at the entry expression. |
| cancellation and interruption | N/A — the validator CLI has no cancellation protocol and this change adds none. | Governance-validator CLI | Structured source review and AC-2. | No cancellation interface or lifecycle is introduced. | AC-2 | N/A — no cancellation protocol | Not run — implementation not started. |
| resource bounds and backpressure | Applicable — repository records can contain arbitrary Metadata labels and values. | `entries` | AC-1 and AC-2. | Focused and complete real-validator checks exit 0 while retaining duplicate-entry behavior. | AC-1, AC-2 | Not Started | Not run — implementation not started. |
| framework or trust-boundary rejection | Applicable — repository Markdown Metadata is validator input that must retain canonical field/value extraction. | `createMetadataValidator` | AC-1 focused real-validator tests. | Focused tests exit 0 and retain existing duplicate-field diagnostics. | AC-1 | Not Started | Not run — implementation not started. |

## Acceptance Checks [Required]

| Check ID | Subtask | Binary acceptance point | Preconditions or input | Verification method | Exact expected result | Expected evidence | Status | Actual result and evidence |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| AC-1 | T-1 | The real validator continues to reject duplicate active Metadata fields with its canonical diagnostic. | Existing `rejects duplicate active <field> metadata` tests. | `node --test --test-name-pattern "rejects duplicate active" test/validation-boundary-regressions.test.mjs` in `tools/governance-validator`. | Process exits 0 with exactly three passing selected tests and no failed selected test. | Focused Node test report. | Not Started | Not run — implementation not started. |
| AC-2 | T-2 | The complete governance-validator suite preserves existing Metadata and governance-record contracts. | Accepted implementation in the isolated task branch. | `npm test` in `tools/governance-validator`. | Process exits 0 with zero failed tests. | Full package test report. | Not Started | Not run — implementation not started. |
| AC-3 | T-2 | Repository governance validation accepts the ADR/index state and unchanged record contracts. | All task changes present in the isolated task branch. | `npm run validate` in `tools/governance-validator`. | Process exits 0 and reports `Governance validation passed.` | Governance-validation command output. | Not Started | Not run — implementation not started. |
| AC-4 | T-2 | A separate accepted OCR verifies the analyzer outcome. | Source correction, AC-1 through AC-3 passed, and an accepted local SonarQube verification OCR. | The specified local Reliability issue view after the OCR analysis completes. | Zero active Reliability findings remain at the former `metadata-validation.mjs` target locations. | OCR task and issue-view evidence without credentials. | Not Started | Not run — separate OCR not yet proposed. |

## Completion Checklist [Required]

| ID | Item | Completion Criterion | Expected Evidence | Status | Actual Evidence |
| --- | --- | --- | --- | --- | --- |
| A-1 | ADR approved | An eligible non-author approver, approval time, and exact `Approval Evidence: Approve` are recorded. | ADR metadata | Not Started | Not run — awaiting approval. |
| A-2 | Complete task delivered | T-1 through T-2 have implementation evidence and AC-1 through AC-4 are Pass. | Implementation Plan and Acceptance Checks | Not Started | Not run — awaiting implementation and OCR verification. |
| A-3 | Reciprocal ADD link synchronized, when applicable | N/A — the task is not derived from product demand and has no ADD candidate. | Metadata Architecture Source | N/A — no ADD applies | N/A — no product-demand ADD applies. |
| A-4 | Requirement levels satisfied | Every required section is complete, and every conditional trigger is completed or has a specific N/A reason. | Structured document review | Not Started | Not run — terminal review follows implementation. |
| A-5 | Acceptance checks are decidable | Every check has one subtask, input, deterministic method, exact expected result, and evidence. | Acceptance Checks table | Not Started | Not run — terminal review follows implementation. |
| A-6 | Engineering exceptions governed, when applicable | N/A — no engineering exception is planned. | Engineering Exceptions section | N/A — no exception applies | N/A — no engineering rule is exceeded or waived. |
| A-7 | Contract and baseline risks covered, when applicable | TC-1 through TC-2 map to checks, and every applicable risk reaches Pass before completion. | Traceability, matrix, and command reports | Not Started | Not run — awaiting implementation and analyzer verification. |
| A-8 | Governance validation passed | The independent validator reports no document or repository validation error. | `npm run validate` output | Not Started | Not run — draft validation has not run. |

## Supporting Notes [Optional]

The other twelve overall Reliability findings are explicitly deferred so this Metadata-entry slice remains independently reviewable. The overall count can remain nonzero after this slice; AC-4 concerns only the two `metadata-validation.mjs` target locations.

## Archival [Conditionally Required — Decision Status is `Rejected`, or Decision Status is `Deprecated` or `Superseded` and Implementation Status is final]

The record is Proposed and not archival-eligible. If a later rejection, deprecation, or supersession triggers archival, move it under `docs/adr/archive/`, update all governed-file markers and references in the same change, and update its single index row.

## Change Log [Required]

| Date | Change | Author |
| --- | --- | --- |
| 2026-08-31 | Drafted the Full ADR for the two overall SonarQube Reliability findings in Metadata-entry recognition. | @codex |
