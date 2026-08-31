# ADR-0011: Metadata Entry Recognition Reliability

## Metadata [Required]

- **Decision Status**: Proposed
- **Implementation Status**: Not Started
- **Date**: 2026-08-31
- **Author**: @codex
- **Decision Owner**: @linhai
- **Required Approver**: @linhai
- **Record Scope**: Project
- **Approver [Conditionally Required — Decision Status is or has been `Accepted`]**: Pending — reapproval required
- **Approval Time [Conditionally Required — Decision Status is or has been `Accepted`]**: Pending — reapproval required
- **Approval Evidence [Conditionally Required — Decision Status is or has been `Accepted`]**: Pending — reapproval required
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
- **Related [Optional]**: Local SonarQube overall Reliability issue list for project `koduck`, observed 2026-08-31 after processed version `2bfdafd` retained one `metadata-validation.mjs` field-suffix finding at L17.
- **Architecture Source [Conditionally Required — product demand]**: N/A — corrective governance-validator work requested from the local SonarQube result, not derived from product demand
- **Supersedes [Conditionally Required — this ADR replaces another]**: N/A — no ADR is replaced
- **Superseded By [Conditionally Required — this ADR is replaced]**: None

## Requirement Level Legend [Required]

- **`[Required]`**: The section or field always applies and MUST remain present with complete, verifiable content. Use `None — <reason>` only when the template explicitly permits an empty result; never leave it blank.
- **`[Conditionally Required — <trigger>]`**: The section or field MUST be completed when its stated trigger applies. When the trigger does not apply, retain `N/A — <reason>` unless the template explicitly instructs removal or retention as inactive future-lifecycle guidance. A missing trigger assessment is incomplete content.
- **`[Optional]`**: The section may be removed without affecting acceptance, execution, completion, or verification. If retained, it MUST be accurate and complete; optional content MUST NOT substitute for required evidence.

Unlabeled fields inside a `[Required]` section are required.

## Context And Problem Statement [Required]

The local SonarQube overall Reliability view initially reported two Open Medium findings at `tools/governance-validator/lib/metadata-validation.mjs` lines 12 and 13: the Metadata-section entry recognizer's unbounded whole-line expression and the requirement-level suffix normalizer's unbounded character-class expression. The first accepted implementation removed the whole-line expression, but processed version `2bfdafd` retains the suffix-normalizer finding at its moved line 17.

`entries` supplies field/value records to every metadata uniqueness and lifecycle check. The established `rejects duplicate active <field> metadata` test family exercises its observable contract for real Metadata fields. The analyzer findings are the red baseline: that existing behavior already passes before the correction, so source-text or artificial timing assertions would be brittle and are prohibited.

## Scope [Required]

In scope:

- Replace the remaining reported Metadata requirement-level suffix normalizer in `metadataEntry` with bounded delimiter-aware parsing that preserves valid active field extraction.
- Preserve the current behavior that strips requirement-level suffixes from field labels, trims field and value content, and ignores non-entry lines outside the real Metadata section.
- Verify existing duplicate-active-metadata behavior, the package suite, and governance validation; verify the remaining target analyzer finding through a separately accepted local SonarQube OCR.

Out of scope:

- Changing lifecycle rules, duplicate-field diagnostics, required-field semantics, metadata schema, scanner configuration, issue workflow, credentials, or the twelve other Reliability findings in `relationship-validation.mjs` and `validate.mjs`.
- Changing `mermaid-validation.mjs`, whose preceding reliability correction is complete under ADR-0010.
- Submitting a SonarQube analysis; that operational step requires a separate accepted OCR after source verification.

## Tensions, Constraints, And Open Questions [Required]

### Identified Tensions [Conditionally Required — competing goals or trade-offs exist]

| ID | Tension | Impact | Decision |
| --- | --- | --- | --- |
| TN-1 | Metadata lines require structural parsing, but a broad parser rewrite could alter field normalization or accept non-metadata prose. | Changing field/value boundaries can invalidate lifecycle and duplicate-field checks; retaining the suffix expression leaves one analyzer finding. | Replace only the remaining suffix normalizer with an explicit bounded delimiter check; retain section selection and all other normalization. |

### Constraints [Required]

- This is a Full ADR because the validator enforces repository-wide governance-record contracts and consumes repository-controlled Markdown.
- No dependency, public contract, record schema, scanner configuration, issue state, or credential handling change is permitted.
- The remaining analyzer finding is the red static-analysis baseline; no source-text assertion, artificial timing threshold, or test that already passes before correction may be presented as a newly failing behavioral regression.
- The modified governed JavaScript file must cite this ADR at its first legal comment position.

### Open Questions [Conditionally Required — material questions exist or were resolved during drafting]

None — the remaining target is owned by one field-normalization path within one primary implementation boundary.

## Decision Drivers [Required]

1. **Remove the remaining reported finding**: authorized reanalysis must have zero active Reliability findings at the Metadata field-suffix location.
2. **Preserve active Metadata semantics**: duplicate active fields remain detected with their canonical field names and trimmed values.
3. **Keep ownership narrow**: no other Reliability owner enters this decision.

## Options Considered [Required]

### Option: Bounded Metadata requirement-suffix recognition

Recognize a trailing requirement-level suffix from its explicit opening and closing delimiters rather than an unbounded character-class expression.

Pros:

- Removes the remaining analyzer-reported suffix expression while preserving `metadataEntry` output.
- Keeps the correction local to the existing field-normalization path.

Cons:

- Requires precise preservation of suffix removal for valid active Metadata fields.

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

**Selected option**: Bounded Metadata requirement-suffix recognition.

**Rationale**: `metadataEntry` now owns field normalization. A local delimiter-aware suffix step removes the remaining reported form while the established real validator tests preserve the observable uniqueness contract.

### Consequences [Required]

Positive:

- The Metadata field normalizer no longer uses the remaining analyzer-reported potentially super-linear expression.
- Existing active-field normalization and duplicate detection remain guarded by real validator tests.

Negative:

- Twelve unrelated overall Reliability findings remain for separately scoped decisions.

Mitigations:

- Touch only `metadataEntry`, run focused and full deterministic checks, and use a separately accepted OCR for analyzer verification.

## Implementation Plan [Required]

**Complete task outcome**: `metadataEntry` replaces the remaining reported requirement-level suffix expression with bounded delimiter-aware parsing while preserving active Metadata field extraction and duplicate detection; after separately authorized reanalysis, no active Reliability finding remains at the remaining Metadata target location.

**Primary implementation boundary**: `tools/governance-validator/lib/metadata-validation.mjs` > `createMetadataValidator` > `metadataEntry` requirement-level suffix normalization.

Allowed subtask statuses: `Not Started`, `In Progress`, `Blocked`, `Complete`, or `N/A — <specific reason>`.

| ID | Objective or deliverable | Included scope | Status | Actual implementation evidence |
| --- | --- | --- | --- | --- |
| T-1 | Replace the remaining Metadata requirement-level suffix expression with bounded delimiter-aware parsing. | `createMetadataValidator` > `metadataEntry` in `tools/governance-validator/lib/metadata-validation.mjs`. | Not Started | Pre-reapproval `2bfdafd` removed only the entry expression and left the analyzer-reported suffix expression active at L17. |
| T-2 | Verify active Metadata duplicate detection, the complete package suite, and governance documentation contracts. | Existing Node.js tests and `npm` scripts in `tools/governance-validator`. | Not Started | Pre-reapproval checks covered the partial correction; rerun after the remaining source correction. |

**Affected paths**: `tools/governance-validator/lib/metadata-validation.mjs`; `tools/governance-validator/test/validation-boundary-regressions.test.mjs`; `docs/adr/ADR-0011-metadata-entry-recognition-reliability.md`; `docs/adr/INDEX.md`.

### Stable Implementation Touchpoints [Conditionally Required — source or configuration implementation]

| Path | Stable symbol or contract anchor | Key code excerpt, when needed | Purpose | Source revision |
| --- | --- | --- | --- | --- |
| `tools/governance-validator/lib/metadata-validation.mjs` | `createMetadataValidator` > `metadataEntry` | N/A — stable symbol is sufficient | Normalizes the field name by removing its optional trailing requirement-level suffix. | `2bfdafd` |
| `tools/governance-validator/test/validation-boundary-regressions.test.mjs` | `rejects duplicate active <field> metadata` | N/A — stable test anchor is sufficient | Exercises duplicate active Metadata entries through the real validator. | `89f51c3` |

**Migration and rollback strategy [Conditionally Required — this replaces or changes existing behavior]**: Replace only requirement-level suffix recognition; stop if focused duplicate-metadata behavior or the complete package suite fails. Rollback is a Git revert of the remaining-correction commit, restoring the prior suffix expression; no data or runtime migration is involved.

### Engineering Exceptions [Conditionally Required — an engineering rule is exceeded or waived]

N/A — the localized replacement does not exceed or waive a software-engineering rule.

## Contract-To-Check Traceability [Conditionally Required — source or configuration implementation]

| Clause ID | Authoritative contract path and heading | Exact normative requirement | Acceptance check or deterministic test IDs | Explicit coverage method |
| --- | --- | --- | --- | --- |
| TC-1 | `tools/governance-validator/lib/metadata-validation.mjs` — `metadataEntry` | Only active Metadata list entries with a bold field label and colon separator yield field/value records; a trailing requirement-level suffix is removed and both values are trimmed. | AC-1, AC-2 | The real validator runs duplicate-entry fixtures and the complete package suite. |
| TC-2 | `tools/governance-validator/lib/metadata-validation.mjs` — `validateUniqueMetadata` | Every repeated active canonical Metadata field produces the existing duplicate-field diagnostic. | AC-1, AC-2 | The focused duplicate-active-metadata tests assert status 1 and the canonical diagnostic for each active-field scenario. |

## Risk Coverage Matrix [Conditionally Required — source or configuration implementation]

| Risk dimension | Applicability and scenario, or specific N/A reason | Owning boundary | Deterministic verification method | Exact expected result | Acceptance check IDs | Status | Actual evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| concurrency and ordering | N/A — entry extraction processes one document synchronously and the replacement adds no shared state. | `entries` | Structured source review and AC-2. | No shared mutable state or ordering contract is introduced. | AC-2 | N/A — no concurrent behavior | Source review of `2bfdafd` found only local variables and ordered line iteration; AC-2 passed. |
| timeout and deadline | Applicable — a repository Metadata line may contain arbitrarily long requirement-level suffix text. | `metadataEntry` | AC-2 package suite and separately authorized analyzer reanalysis. | Package suite exits 0, and reanalysis has no active finding at the reported Metadata suffix location. | AC-2, AC-4 | Not Started | Processed version `2bfdafd` retains one Open Medium Reliability finding at line 17. |
| cancellation and interruption | N/A — the validator CLI has no cancellation protocol and this change adds none. | Governance-validator CLI | Structured source review and AC-2. | No cancellation interface or lifecycle is introduced. | AC-2 | N/A — no cancellation protocol | Source review of `2bfdafd` confirms no cancellation interface or lifecycle was added; AC-2 passed. |
| resource bounds and backpressure | Applicable — repository records can contain arbitrary Metadata labels and suffixes. | `metadataEntry` | AC-1 and AC-2. | Focused and complete real-validator checks exit 0 while retaining duplicate-entry behavior. | AC-1, AC-2 | Not Started | Rerun after the remaining suffix correction. |
| framework or trust-boundary rejection | Applicable — repository Markdown Metadata is validator input that must retain canonical field/value extraction. | `createMetadataValidator` | AC-1 focused real-validator tests. | Focused tests exit 0 and retain existing duplicate-field diagnostics. | AC-1 | Not Started | Rerun after the remaining suffix correction. |

## Acceptance Checks [Required]

| Check ID | Subtask | Binary acceptance point | Preconditions or input | Verification method | Exact expected result | Expected evidence | Status | Actual result and evidence |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| AC-1 | T-1 | The real validator continues to reject duplicate active Metadata fields with its canonical diagnostic. | Existing `rejects duplicate active <field> metadata` tests. | `node --test --test-name-pattern "rejects duplicate active" test/validation-boundary-regressions.test.mjs` in `tools/governance-validator`. | Process exits 0 with exactly three passing selected tests and no failed selected test. | Focused Node test report. | Not Started | Pre-reapproval tests passed, but the remaining suffix correction must rerun them. |
| AC-2 | T-2 | The complete governance-validator suite preserves existing Metadata and governance-record contracts. | Reapproved implementation in the isolated task branch. | `npm test` in `tools/governance-validator`. | Process exits 0 with zero failed tests. | Full package test report. | Not Started | Rerun after reapproved source correction. |
| AC-3 | T-2 | Repository governance validation accepts the ADR/index state and unchanged record contracts. | All task changes present in the isolated task branch. | `npm run validate` in `tools/governance-validator`. | Process exits 0 and reports `Governance validation passed.` | Governance-validation command output. | Not Started | Rerun after reapproved source correction. |
| AC-4 | T-2 | A separate accepted OCR verifies the analyzer outcome. | Reapproved source correction, AC-1 through AC-3 passed, and an accepted local SonarQube verification OCR. | The specified local Reliability issue view after the OCR analysis completes. | Zero active Reliability findings remain at the remaining `metadata-validation.mjs` suffix target location. | OCR task and issue-view evidence without credentials. | Not Started | OCR-0007 verified that the partial `2bfdafd` correction is insufficient; a new OCR must verify the reapproved source correction. |

## Completion Checklist [Required]

| ID | Item | Completion Criterion | Expected Evidence | Status | Actual Evidence |
| --- | --- | --- | --- | --- | --- |
| A-1 | ADR approved | An eligible non-author approver, approval time, and exact `Approval Evidence: Approve` are recorded. | ADR metadata | Not Started | The prior @linhai approval at 2026-08-31T22:58:12+08:00 was invalidated by the clarified remaining-suffix scope. |
| A-2 | Complete task delivered | T-1 through T-2 have implementation evidence and AC-1 through AC-4 are Pass. | Implementation Plan and Acceptance Checks | Not Started | Not run — awaiting implementation and OCR verification. |
| A-3 | Reciprocal ADD link synchronized, when applicable | N/A — the task is not derived from product demand and has no ADD candidate. | Metadata Architecture Source | N/A — no ADD applies | N/A — no product-demand ADD applies. |
| A-4 | Requirement levels satisfied | Every required section is complete, and every conditional trigger is completed or has a specific N/A reason. | Structured document review | Not Started | Not run — terminal review follows implementation. |
| A-5 | Acceptance checks are decidable | Every check has one subtask, input, deterministic method, exact expected result, and evidence. | Acceptance Checks table | Not Started | Not run — terminal review follows implementation. |
| A-6 | Engineering exceptions governed, when applicable | N/A — no engineering exception is planned. | Engineering Exceptions section | N/A — no exception applies | N/A — no engineering rule is exceeded or waived. |
| A-7 | Contract and baseline risks covered, when applicable | TC-1 through TC-2 map to checks, and every applicable risk reaches Pass before completion. | Traceability, matrix, and command reports | Not Started | Not run — awaiting implementation and analyzer verification. |
| A-8 | Governance validation passed | The independent validator reports no document or repository validation error. | `npm run validate` output | Not Started | Rerun after the reapproval revision is accepted. |

## Supporting Notes [Optional]

Twelve unrelated overall Reliability findings are deferred so this remaining Metadata suffix slice remains independently reviewable. OCR-0007 confirmed that the whole-entry correction cleared while the suffix normalizer still reports at L17.

## Archival [Conditionally Required — Decision Status is `Rejected`, or Decision Status is `Deprecated` or `Superseded` and Implementation Status is final]

The record is Proposed and not archival-eligible. If a later rejection, deprecation, or supersession triggers archival, move it under `docs/adr/archive/`, update all governed-file markers and references in the same change, and update its single index row.

## Change Log [Required]

| Date | Change | Author |
| --- | --- | --- |
| 2026-08-31 | Drafted the Full ADR for the two overall SonarQube Reliability findings in Metadata-entry recognition. | @codex |
| 2026-08-31 | Accepted by @linhai with approval evidence `Approve` at 2026-08-31T22:58:12+08:00. | @codex |
| 2026-08-31 | Implemented delimiter-aware Metadata-entry recognition in `2bfdafd`; focused and full checks passed. | @codex |
| 2026-08-31 | OCR-0007 showed that the remaining suffix normalizer reports at L17; clarified the source scope and invalidated the prior approval. Previous approver: @linhai; previous approval time: 2026-08-31T22:58:12+08:00; previous approval evidence: `Approve`. | @codex |
