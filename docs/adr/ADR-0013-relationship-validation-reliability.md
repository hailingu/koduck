# ADR-0013: Relationship Validation Reliability

## Metadata [Required]

- **Decision Status**: Accepted
- **Implementation Status**: Not Started
- **Date**: 2026-09-01
- **Author**: @codex
- **Decision Owner**: @linhai
- **Required Approver**: @linhai
- **Record Scope**: Project
- **Approver [Conditionally Required — Decision Status is or has been `Accepted`]**: @linhai
- **Approval Time [Conditionally Required — Decision Status is or has been `Accepted`]**: 2026-09-01T09:42:39+08:00
- **Approval Evidence [Conditionally Required — Decision Status is or has been `Accepted`]**: Approve
- **Rejector [Conditionally Required — Decision Status is `Rejected`]**: N/A — Decision Status is Accepted
- **Rejection Time [Conditionally Required — Decision Status is `Rejected`]**: N/A — Decision Status is Accepted
- **Rejection Evidence [Conditionally Required — Decision Status is `Rejected`]**: N/A — Decision Status is Accepted
- **Retired By [Conditionally Required — Decision Status is `Deprecated` or `Superseded`]**: N/A — record is not retired
- **Retirement Time [Conditionally Required — Decision Status is `Deprecated` or `Superseded`]**: N/A — record is not retired
- **Retirement Evidence [Conditionally Required — Decision Status is `Deprecated` or `Superseded`]**: N/A — record is not retired
- **Retirement Reason [Conditionally Required — Decision Status is `Deprecated` or `Superseded`]**: N/A — record is not retired
- **Blocked From [Conditionally Required — Implementation Status is `Blocked`]**: N/A — implementation is not blocked
- **Blocker And Evidence [Conditionally Required — Implementation Status is `Blocked`]**: N/A — implementation is not blocked
- **Blocker Owner [Conditionally Required — Implementation Status is `Blocked`]**: N/A — implementation is not blocked
- **Blocker Exit Or Recheck Criterion [Conditionally Required — Implementation Status is `Blocked`]**: N/A — implementation is not blocked
- **Related [Optional]**: Local SonarQube project `koduck` Reliability view observed 2026-09-01: one Medium super-linear regular-expression finding at `tools/governance-validator/lib/relationship-validation.mjs:L117`, and two Low findings at `:L151` and `:L152` requesting `String#replaceAll()`.
- **Architecture Source [Conditionally Required — product demand]**: N/A — corrective governance-validator work requested from local SonarQube results, not derived from product demand
- **Supersedes [Conditionally Required — this ADR replaces another]**: N/A — no ADR is replaced
- **Superseded By [Conditionally Required — this ADR is replaced]**: None

## Requirement Level Legend [Required]

- **`[Required]`**: The section or field always applies and MUST remain present with complete, verifiable content. Use `None — <reason>` only when the template explicitly permits an empty result; never leave it blank.
- **`[Conditionally Required — <trigger>]`**: The section or field MUST be completed when its stated trigger applies. When the trigger does not apply, retain `N/A — <reason>` unless the template explicitly instructs removal or retention as inactive future-lifecycle guidance. A missing trigger assessment is incomplete content.
- **`[Optional]`**: The section may be removed without affecting acceptance, execution, completion, or verification. If retained, it MUST be accurate and complete; optional content MUST NOT substitute for required evidence.

Unlabeled fields inside a `[Required]` section are required.

## Context And Problem Statement [Required]

The processed local SonarQube Reliability view reports twelve open issues. Three have one primary implementation boundary: `tools/governance-validator/lib/relationship-validation.mjs`. The current title comparison uses a whole-document multiline regular expression whose optional whitespace and greedy title capture receive a Medium super-linear-backtracking finding. The same comparison loop uses two global regular-expression replacements to remove literal Markdown backticks from index and record metadata, which produces two Low `String#replaceAll()` findings.

The other nine Reliability findings are in `tools/governance-validator/validate.mjs`, a separate implementation boundary. They are expressly deferred to the next ADR. This record is limited to preserving the relationship validator's existing index-title and metadata-comparison contract while clearing only its three scoped analyzer rows.

The local analyzer report is the static red baseline. Focused tests will exercise the real validator contract; no test will assert source text, source layout, or an artificial timing threshold.

## Scope [Required]

In scope:

- Replace the title-comparison regular expression with bounded, line-oriented record-H1 recognition that accepts the existing ADR, OCR, ADD, and Lightweight ADR heading forms.
- Replace the two literal-backtick global regular-expression replacements with `String#replaceAll()` while retaining comparison results.
- Add focused real-validator regressions for index-title disagreement and metadata values that differ only by Markdown backticks; run package, governance, and separately authorized local analyzer verification.

Out of scope:

- Changes to `tools/governance-validator/validate.mjs`, including its nine remaining Reliability findings.
- Changes to relationship, index, lifecycle, file-resolution, scanner, credential, or public governance contracts beyond the three scoped implementation details.
- New dependencies, generated artifacts, source-text assertions, artificial timing tests, or changes to local SonarQube configuration.

## Tensions, Constraints, And Open Questions [Required]

### Identified Tensions [Conditionally Required — competing goals or trade-offs exist]

| ID | Tension | Impact | Decision |
| --- | --- | --- | --- |
| TN-1 | The current concise expression recognizes record titles, while unbounded whole-document regular-expression matching is flagged for reliability. | A simplistic replacement could stop recognizing valid record headings or change the index-title diagnostic. | Parse candidate H1 lines with explicit delimiters and preserve the title comparison's existing result and diagnostic. |
| TN-2 | The analyzer requests modern literal replacement, while Markdown backtick normalization is an existing cross-file comparison behavior. | A replacement API change could accidentally alter the normalization rule. | Use `replaceAll("`", "")` with the same literal target and empty replacement on both values. |

### Constraints [Required]

- This is a Full ADR because the project-wide governance validator enforces repository decision-record contracts.
- The source change must stay within the one independently reviewable primary boundary `relationship-validation.mjs`; the remaining `validate.mjs` findings require a later ADR after this record becomes terminal.
- The first legal comment in the modified governed JavaScript file must cite this ADR.
- The title parser must not rely on a whole-document greedy regular-expression capture; it must preserve recognition of valid `ADR`, `OCR`, `ADD`, and `Lightweight ADR` H1 forms.
- Local SonarQube verification is an operational action and requires a separate accepted OCR after the source checks pass; no credential may be recorded in this ADR or its evidence.

### Open Questions [Conditionally Required — material questions exist or were resolved during drafting]

None — the three scoped analyzer findings, source ownership, and preservation boundary are concrete.

## Decision Drivers [Required]

1. **Reliability**: Remove the flagged whole-document title matcher and retain predictable record-heading recognition for arbitrary repository Markdown.
2. **Contract preservation**: Keep index title mismatch diagnostics and Markdown-backtick normalization intact.
3. **Isolated ownership**: Do not combine the separately reviewable `validate.mjs` remediation with this source slice.

## Options Considered [Required]

### Option: Bounded heading parser and literal replacement API

Scan record lines, parse only the recognized heading delimiters, and use `replaceAll` for the two literal-backtick normalizations.

Pros:

- Removes all three scoped analyzer findings without broadening the module boundary.
- Makes the heading grammar and literal normalization explicit.

Cons:

- Adds a small parser helper instead of retaining one compact expression.

### Option: Keep the existing implementation

Retain the multiline regular expression and global regular-expression replacements.

Pros:

- Requires no source changes.

Cons:

- Leaves the three open Reliability findings unresolved.

### Option: Combine all twelve Reliability findings

Modify both `relationship-validation.mjs` and `validate.mjs` in one implementation change.

Pros:

- Could reduce the aggregate analyzer count sooner.

Cons:

- Combines two independent primary implementation boundaries and violates the ADR scope rule.

## Decision [Required]

**Selected option**: Bounded heading parser and literal replacement API.

**Rationale**: A line-oriented parser confines title recognition to one candidate line at a time and preserves the existing accepted heading forms and comparison output. Replacing a literal global regular expression with `replaceAll` is behaviorally equivalent for the two Markdown-backtick normalizations. Together these changes are the smallest coherent remediation in the relationship-validation boundary.

### Consequences [Required]

Positive:

- The three scoped Reliability findings can be cleared without changing the validator's relationship ownership or public invocation.
- Valid record title and backtick-normalized metadata comparisons remain explicitly covered by behavioral tests.

Negative:

- The module gains a small non-exported heading parser.
- Nine Reliability findings remain open in `validate.mjs` until its later ADR is authorized and completed.

Mitigations:

- Keep the existing index diagnostic wording and field-comparison inputs; add focused fixtures, execute the package and governance checks, then use a separate accepted OCR for scoped analyzer confirmation.

## Implementation Plan [Required]

**Complete task outcome**: `relationship-validation.mjs` recognizes valid record H1 titles without a whole-document greedy matcher, normalizes literal Markdown backticks through `replaceAll`, preserves index title and metadata comparison behavior, and has zero active scoped local-SonarQube Reliability rows after separately authorized verification.

**Primary implementation boundary**: `tools/governance-validator/lib/relationship-validation.mjs` > `createRelationshipValidator` > `validateIndex` title and authoritative-field comparison paths.

Allowed subtask statuses: `Not Started`, `In Progress`, `Blocked`, `Complete`, or `N/A — <specific reason>`.

| ID | Objective or deliverable | Included scope | Status | Actual implementation evidence |
| --- | --- | --- | --- | --- |
| T-1 | Replace the title-comparison matcher with bounded record-H1 parsing and update the governed-file marker. | `tools/governance-validator/lib/relationship-validation.mjs` title path and a focused index-title regression. | Not Started | Pending — implementation awaits acceptance. |
| T-2 | Replace literal-backtick normalizers with `replaceAll` and verify relationship comparison contracts. | `tools/governance-validator/lib/relationship-validation.mjs` metadata values; focused index test; package and governance checks. | Not Started | Pending — implementation awaits acceptance. |
| T-3 | Confirm that local SonarQube no longer reports the three scoped Reliability rows. | A separately accepted OCR and the local `koduck` Reliability issue view. | Not Started | Pending — operational verification awaits completed source checks and an accepted OCR. |

**Affected paths**: `tools/governance-validator/lib/relationship-validation.mjs`; `tools/governance-validator/test/index-path-validation.test.mjs`; `docs/adr/ADR-0013-relationship-validation-reliability.md`; `docs/adr/INDEX.md`; a later OCR path under `docs/adr/ocr/`.

### Stable Implementation Touchpoints [Conditionally Required — source or configuration implementation]

| Path | Stable symbol or contract anchor | Key code excerpt, when needed | Purpose | Source revision |
| --- | --- | --- | --- | --- |
| `tools/governance-validator/lib/relationship-validation.mjs` | `createRelationshipValidator` > `validateIndex` > Title comparison | N/A — stable symbols are sufficient | Reads the indexed and record H1 titles and emits the canonical index-title disagreement diagnostic. | `9a38dc2` |
| `tools/governance-validator/lib/relationship-validation.mjs` | `createRelationshipValidator` > `validateIndex` > authoritative field comparisons | N/A — stable symbols are sufficient | Normalizes Markdown backticks before comparing indexed and record relationship metadata. | `9a38dc2` |
| `tools/governance-validator/test/index-path-validation.test.mjs` | focused relationship-index regression tests | N/A — stable test anchors will be added before implementation. | Exercises record-title disagreement and equivalence when values differ only by Markdown backticks. | `9a38dc2` |

**Migration and rollback strategy [Conditionally Required — this replaces or changes existing behavior]**: Replace only the title-recognition and literal-normalization implementation details. Stop if a focused validator regression, the package suite, or governance validation fails. Roll back with a Git revert of the source commit; no data, runtime, or external-system migration is involved.

### Engineering Exceptions [Conditionally Required — an engineering rule is exceeded or waived]

N/A — the bounded helper and focused behavioral tests follow the software-engineering standard without an exception.

## Contract-To-Check Traceability [Conditionally Required — source or configuration implementation]

| Clause ID | Authoritative contract path and heading | Exact normative requirement | Acceptance check or deterministic test IDs | Explicit coverage method |
| --- | --- | --- | --- | --- |
| RVC-1 | `tools/governance-validator/lib/relationship-validation.mjs` — `createRelationshipValidator` > `validateIndex` > Title comparison | When a referenced record has a recognized record H1 title and its index `Title` differs, validation MUST emit the existing `index Title disagrees with record` diagnostic. | AC-1, AC-3 | A focused temporary-repository fixture changes the indexed record's recognized H1 title and asserts the existing diagnostic; the complete suite preserves all contracts. |
| RVC-2 | `tools/governance-validator/lib/relationship-validation.mjs` — `createRelationshipValidator` > `validateIndex` > authoritative field comparisons | Indexed and record authoritative values MUST compare after removing every literal Markdown backtick from each value. | AC-2, AC-3 | A focused temporary-repository fixture uses equivalent values that differ only by Markdown backticks and asserts successful validation. |

## Risk Coverage Matrix [Conditionally Required — source or configuration implementation]

| Risk dimension | Applicability and scenario, or specific N/A reason | Owning boundary | Deterministic verification method | Exact expected result | Acceptance check IDs | Status | Actual evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| concurrency and ordering | N/A — `validateIndex` validates each record synchronously and the change introduces no shared state or cross-record ordering contract. | `createRelationshipValidator` | Structured source review and AC-3. | No shared mutable state or ordering behavior is introduced. | AC-3 | N/A — no concurrent behavior | Not run — implementation not started. |
| timeout and deadline | Applicable — repository Markdown may include a malformed or oversized candidate heading. | `validateIndex` title parser | AC-1 focused behavior plus AC-3 complete suite. | Focused and complete checks exit 0 without changing title-mismatch semantics. | AC-1, AC-3 | Not Started | Pending — implementation awaits acceptance. |
| cancellation and interruption | N/A — the validator CLI has no cancellation protocol and this synchronous parser adds none. | Governance-validator CLI | Structured source review and AC-3. | No cancellation interface or lifecycle is introduced. | AC-3 | N/A — no cancellation protocol | Not run — implementation not started. |
| resource bounds and backpressure | Applicable — the bounded parser must process repository-controlled H1 text without whole-document greedy capture. | `validateIndex` title parser | AC-1 focused behavior and AC-3 complete suite. | The parser preserves the canonical diagnostic for a recognized record heading. | AC-1, AC-3 | Not Started | Pending — implementation awaits acceptance. |
| framework or trust-boundary rejection | Applicable — Markdown index and record content are untrusted repository inputs whose normalized relationships must be rejected on disagreement. | `createRelationshipValidator` > `validateIndex` | AC-1 and AC-2 focused temporary-repository tests. | A title disagreement exits 1 with the canonical diagnostic; values differing only by backticks validate successfully. | AC-1, AC-2 | Not Started | Pending — implementation awaits acceptance. |

## Acceptance Checks [Required]

| Check ID | Subtask | Binary acceptance point | Preconditions or input | Verification method | Exact expected result | Expected evidence | Status | Actual result and evidence |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| AC-1 | T-1 | The real validator rejects an index whose referenced ADR H1 title differs from the index title. | A focused `index-path-validation` temporary repository with a valid `# ADR-0001: <different title>` H1 and unchanged index title. | `node --test --test-name-pattern "index title" test/index-path-validation.test.mjs` in `tools/governance-validator`. | Process exits 0; exactly the selected test passes; its inner validator result exits 1 and contains `index Title disagrees with record`. | Focused Node test report. | Not Started | Pending — implementation awaits acceptance. |
| AC-2 | T-2 | The real validator accepts equivalent index and record authoritative values that differ only by literal Markdown backticks. | A focused `index-path-validation` temporary repository whose `Architecture Source` index and record values are equivalent after backtick removal. | `node --test --test-name-pattern "backticks" test/index-path-validation.test.mjs` in `tools/governance-validator`. | Process exits 0; exactly the selected test passes; its inner validator result exits 0. | Focused Node test report. | Not Started | Pending — implementation awaits acceptance. |
| AC-3 | T-2 | The complete governance-validator suite and repository governance validation preserve all existing contracts. | Approved source and test changes in the isolated task branch. | Run `npm test`, then `npm run validate`, in `tools/governance-validator`. | Both commands exit 0; the test suite has zero failures and validation reports `Governance validation passed.` | Complete package and governance-validation outputs. | Not Started | Pending — implementation awaits acceptance. |
| AC-4 | T-3 | A separately accepted OCR confirms that all three scoped analyzer rows are absent. | AC-1 through AC-3 passed, source revision committed, and an accepted local SonarQube verification OCR. | Run the OCR-defined local scanner command and inspect the scoped Reliability issue view after processing completes. | Zero active Reliability rows remain for `relationship-validation.mjs` title matcher and both literal-backtick replacement findings. | OCR task identifier and issue-view evidence without credentials. | Not Started | Pending — implementation awaits acceptance. |

## Completion Checklist [Required]

| ID | Item | Completion Criterion | Expected Evidence | Status | Actual Evidence |
| --- | --- | --- | --- | --- | --- |
| A-1 | ADR approved | An eligible non-author approver, approval time, and exact `Approval Evidence: Approve` are recorded. | ADR metadata | Complete | @linhai approved at 2026-09-01T09:42:39+08:00 with approval evidence `Approve`. |
| A-2 | Complete task delivered | T-1 through T-3 have implementation evidence and AC-1 through AC-4 are Pass. | Implementation Plan and Acceptance Checks | Not Started | Pending — implementation has not started. |
| A-3 | Reciprocal ADD link synchronized, when applicable | N/A — this task is not derived from product demand and has no ADD candidate. | Metadata Architecture Source | N/A — no ADD applies | N/A — no product-demand ADD applies. |
| A-4 | Requirement levels satisfied | Every required section is complete, and every conditional trigger is completed or has a specific N/A reason. | Structured document review | Not Started | Pending — terminal review follows implementation. |
| A-5 | Acceptance checks are decidable | Every check has one subtask, input, deterministic method, exact expected result, and evidence. | Acceptance Checks table | Not Started | Pending — approval review required. |
| A-6 | Engineering exceptions governed, when applicable | N/A — no engineering exception is planned. | Engineering Exceptions section | N/A — no exception applies | N/A — no engineering rule is exceeded or waived. |
| A-7 | Contract and baseline risks covered, when applicable | RVC-1 through RVC-2 map to checks, and every applicable risk reaches Pass before completion. | Traceability, matrix, and command reports | Not Started | Pending — implementation has not started. |
| A-8 | Governance validation passed | The independent validator reports no document or repository validation error. | `npm run validate` output | Not Started | Pending — validation follows implementation. |

## Supporting Notes [Optional]

The remaining nine Reliability findings in `tools/governance-validator/validate.mjs` are intentionally deferred. They must be governed by a new ADR only after this ADR reaches a truthful terminal implementation status.

## Archival [Conditionally Required — Decision Status is `Rejected`, or Decision Status is `Deprecated` or `Superseded` and Implementation Status is final]

The record is Proposed, so archival is inactive future-lifecycle guidance. If a qualifying retirement occurs, move this file under `docs/adr/archive/`, update its index row and every marker or reciprocal reference in the same change, retain `Superseded By: None` unless a replacement is identified, and confirm no active reference remains to the pre-archive path.

## Change Log [Required]

| Date | Change | Author |
| --- | --- | --- |
| 2026-09-01 | Drafted the isolated relationship-validation Reliability remediation for the three scoped local SonarQube findings. | @codex |
| 2026-09-01 | Accepted by @linhai with approval evidence `Approve`. | @linhai |
