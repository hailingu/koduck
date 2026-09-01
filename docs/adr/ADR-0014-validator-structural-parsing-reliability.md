# ADR-0014: Validator Structural Parsing Reliability

## Metadata [Required]

- **Decision Status**: Accepted
- **Implementation Status**: In Progress
- **Date**: 2026-09-01
- **Author**: @codex
- **Decision Owner**: @linhai
- **Required Approver**: @linhai
- **Record Scope**: Project
- **Approver [Conditionally Required — Decision Status is or has been `Accepted`]**: @linhai
- **Approval Time [Conditionally Required — Decision Status is or has been `Accepted`]**: 2026-09-01T10:16:30+08:00
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
- **Related [Optional]**: Local SonarQube project `koduck` overall Reliability view observed 2026-09-01: nine Open findings in `tools/governance-validator/validate.mjs` — eight Medium super-linear regular-expression findings at the then-current L232, L241, L246, L540, L651, L802, L812, and L817, plus one Low `String#replaceAll()` finding at L668. The file is identical between local `dev` revision `9a38dc2` and the processed local analysis revision `f2cc310`.
- **Architecture Source [Conditionally Required — product demand]**: N/A — corrective governance-validator work requested from local SonarQube results, not derived from product demand
- **Supersedes [Conditionally Required — this ADR replaces another]**: N/A — no ADR is replaced
- **Superseded By [Conditionally Required — this ADR is replaced]**: None

## Requirement Level Legend [Required]

- **`[Required]`**: The section or field always applies and MUST remain present with complete, verifiable content. Use `None — <reason>` only when the template explicitly permits an empty result; never leave it blank.
- **`[Conditionally Required — <trigger>]`**: The section or field MUST be completed when its stated trigger applies. When the trigger does not apply, retain `N/A — <reason>` unless the template explicitly instructs removal or retention as inactive future-lifecycle guidance. A missing trigger assessment is incomplete content.
- **`[Optional]`**: The section may be removed without affecting acceptance, execution, completion, or verification. If retained, it MUST be accurate and complete; optional content MUST NOT substitute for required evidence.

Unlabeled fields inside a `[Required]` section are required.

## Context And Problem Statement [Required]

The local SonarQube Reliability view currently reports nine Open findings in one primary implementation boundary: `tools/governance-validator/validate.mjs`. Eight Medium findings identify potentially super-linear regular expressions in level-two-heading extraction, requirement-level normalization, record-path recognition, checklist recognition, and ADR/OCR/ADD filename classification. One Low finding requests `String#replaceAll()` for risk-dimension hyphen normalization.

These helpers consume repository-controlled Markdown and filenames, then provide structural inputs to required-section, lifecycle, supersession, accepted-record, and terminal-record validation. The observable contracts already have behavioral coverage, so the analyzer results are the static red baseline. The implementation must add or extend real-validator contract tests but must not use source-text assertions or artificial timing thresholds as a substitute for behavioral verification.

The base revision `9a38dc2` has 847 physical lines in `validate.mjs`, above the software-engineering standard's 800-line exception limit. This ADR therefore carries an approval-sensitive, bounded engineering exception for this narrow static-analysis correction. The prior relationship-validation correction in ADR-0013 is intentionally not part of this source slice: before its OCR scan, the implementation branch must include terminal ADR-0013 source commit `f2cc310` or its exact equivalent so the three already-cleared unrelated findings do not reappear in the whole-project result.

## Scope [Required]

In scope:

- Replace the eight reported regular-expression operations in `headingTexts`, `sectionName`, `validateRequirementLevels`, `validateSupersession`, `checklistItems`, and the ADR/OCR/ADD dispatch in `validate` with bounded delimiter-aware or line-oriented recognition that preserves their existing accepted and rejected inputs.
- Replace the literal global hyphen normalizer in `normalizeDimension` with `String#replaceAll()` while retaining canonical risk-dimension equivalence.
- Update the first legal governed-file marker in `validate.mjs`, add focused real-validator contract coverage, run the package and governance checks, and use a separately accepted OCR to verify zero active scoped Reliability rows.

Out of scope:

- Integrating, modifying, or re-verifying ADR-0013's `relationship-validation.mjs` correction; its terminal source commit is a prerequisite to whole-project scan verification, not an implementation target here.
- Changes to public governance-record semantics, diagnostic wording, lifecycle policy, scanner configuration, issue state, credentials, generated artifacts, or dependencies.
- Refactoring unrelated validator modules or reducing the pre-existing `validate.mjs` file-size exception through a broad decomposition.

## Tensions, Constraints, And Open Questions [Required]

### Identified Tensions [Conditionally Required — competing goals or trade-offs exist]

| ID | Tension | Impact | Decision |
| --- | --- | --- | --- |
| TN-1 | The current regular expressions compactly recognize document structure, while their unbounded forms are reported for potential backtracking. | A broad rewrite could change accepted Markdown, filenames, or canonical diagnostics; retaining the patterns leaves eight findings open. | Replace only the reported matchers with bounded line- and delimiter-oriented parsing, and exercise the existing validator contracts through focused fixtures. |
| TN-2 | The source file is already above the exception limit, while a file decomposition would broaden a narrow static-analysis correction. | Adding a broad refactor would obscure the nine targeted changes; leaving the exception undocumented would violate the engineering standard. | Permit only the scoped replacements under the recorded exception, cap the resulting file at 900 lines, and require a separate decomposition decision before an unrelated later modification. |
| TN-3 | The local dashboard reflects ADR-0013's terminal source revision, but the required local `dev` base does not yet contain that commit. | A whole-project scan from an implementation branch without the prerequisite could reintroduce three unrelated findings. | Keep ADR-0013 integration out of scope and require `f2cc310` or its exact equivalent before the separately authorized OCR scan. |

### Constraints [Required]

- This is a Full ADR because the project-wide governance validator controls repository decision-record and lifecycle contracts.
- The source change has exactly one primary implementation boundary: the structural-parsing and record-classification paths of `tools/governance-validator/validate.mjs`.
- The first legal comment in the modified JavaScript file must cite this ADR's current repository-relative path.
- Each replacement must preserve the established success state and canonical rejection diagnostics for its affected validator contract; source-text and artificial timing assertions are prohibited.
- Local SonarQube analysis is an operational action that requires a separate accepted OCR after source verification. No credential or token value may be recorded in this ADR, tests, commit, or evidence.
- Before the OCR scan, its input must include ADR-0013 source commit `f2cc310` or an exact equivalent, in addition to this ADR's source commit.

### Open Questions [Conditionally Required — material questions exist or were resolved during drafting]

None — the source ownership, nine scoped findings, prerequisite, and exact verification boundary are recorded above.

## Decision Drivers [Required]

1. **Bounded structural recognition**: Remove every reported unbounded matcher while keeping repository-record validation deterministic for arbitrary Markdown and filenames.
2. **Contract preservation**: Retain required-section, supersession, checklist, risk-dimension, and record-type outcomes and diagnostics.
3. **Narrow ownership**: Complete only the `validate.mjs` reliability slice without reopening the finished relationship-validation boundary.
4. **Governed maintainability**: Make the existing over-limit file condition explicit and constrain it until a dedicated decomposition decision is made.

## Options Considered [Required]

### Option: Bounded structural parsers and literal replacement API

Replace each reported operation with line scanning, explicit delimiter checks, or literal `replaceAll` calls while retaining function ownership and existing contract tests.

Pros:

- Clears all nine scoped analyzer findings within one implementation boundary.
- Makes structural delimiters and literal normalization explicit without changing public contracts.

Cons:

- Introduces more direct parsing code than the current compact regular expressions.
- Leaves the pre-existing file-size exception in place for this intentionally narrow task.

### Option: Suppress or relabel the analyzer findings

Leave `validate.mjs` unchanged and alter SonarQube workflow or configuration.

Pros:

- Avoids source changes.

Cons:

- Retains the reported unbounded operations and changes analyzer workflow outside the primary boundary.

### Option: Combine the relationship and structural-parser corrections

Modify both `relationship-validation.mjs` and `validate.mjs` in this task.

Pros:

- Could submit one complete project scan.

Cons:

- Mixes an already completed, independently reviewable implementation boundary with this one and violates the ADR scope rule.

## Decision [Required]

**Selected option**: Bounded structural parsers and literal replacement API.

**Rationale**: The reported operations all belong to `validate.mjs`'s structural-recognition boundary. Replacing them with bounded parsing keeps their input grammar and validator outcomes explicit, while `replaceAll` precisely preserves literal hyphen normalization. A narrowly scoped engineering exception permits this reliability correction without disguising it as a broader module-decomposition effort.

### Consequences [Required]

Positive:

- The local analyzer can clear all nine remaining scoped Reliability rows without a public governance-contract change.
- Real-validator tests will make accepted and rejected structural inputs explicit.
- The existing file-size condition has an accountable, bounded lifecycle.

Negative:

- `validate.mjs` may remain above the standard limit after this narrow correction.
- The OCR cannot be submitted until ADR-0013's terminal source correction is also present in its input revision.

Mitigations:

- Cap the modified file at 900 physical lines, forbid unrelated scope and dependencies, use focused and complete deterministic checks, and require a separate accepted OCR for scanner confirmation.
- Require the OCR input to include `f2cc310` or its exact equivalent, then record only non-sensitive task and issue-view evidence.

## Implementation Plan [Required]

**Complete task outcome**: `validate.mjs` recognizes its documented structural inputs through bounded parsing and literal `replaceAll` normalization, preserves the established real-validator contracts, stays at or below 900 physical lines under the approved exception, and has zero active scoped local-SonarQube Reliability rows after separately authorized verification of an input that also contains ADR-0013's terminal source correction.

**Primary implementation boundary**: `tools/governance-validator/validate.mjs` > structural Markdown parsing, supersession path recognition, checklist/risk normalization, and ADR/OCR/ADD record classification.

Allowed subtask statuses: `Not Started`, `In Progress`, `Blocked`, `Complete`, or `N/A — <specific reason>`.

| ID | Objective or deliverable | Included scope | Status | Actual implementation evidence |
| --- | --- | --- | --- | --- |
| T-1 | Replace the heading, requirement-level, and supersession-path recognizers with bounded parsing while preserving structural diagnostics. | `headingTexts`, `sectionName`, `validateRequirementLevels`, `recordPathFromValue`, and `validateSupersession` in `validate.mjs`; focused structural and lifecycle tests. | Complete | `536cf5b`, `31328ab`; AC-1 and the supersession portion of AC-2 each passed as one selected real-validator test. |
| T-2 | Replace checklist and risk-dimension operations with bounded/literal normalization while preserving terminal and accepted-record contracts. | `checklistItems` and `normalizeDimension` in `validate.mjs`; focused lifecycle tests. | Complete | `536cf5b`, `f9d08d5`; AC-2 passed with malformed checklist, malformed replacement-path, and hyphen/space-normalized terminal-risk fixtures. |
| T-3 | Replace ADR/OCR/ADD filename dispatch recognition, update the file marker, and verify the complete scoped result. | `validate` record dispatch; focused classification test; package and governance checks; a later accepted local SonarQube OCR. | In Progress | `536cf5b`, `31328ab`; ADR marker, dispatch, and the remaining supersession record-type classifier are bounded. AC-3/AC-4 pass. OCR-0011 restored the `f2cc310` baseline after observing the residual L584 row; a new accepted OCR is required for this source revision. |

**Affected paths**: `tools/governance-validator/validate.mjs`; `tools/governance-validator/test/validate-structure.test.mjs`; `tools/governance-validator/test/validate-lifecycle.test.mjs`; `docs/adr/ADR-0014-validator-structural-parsing-reliability.md`; `docs/adr/INDEX.md`; a later OCR path under `docs/adr/ocr/`.

### Stable Implementation Touchpoints [Conditionally Required — source or configuration implementation]

| Path | Stable symbol or contract anchor | Key code excerpt, when needed | Purpose | Source revision |
| --- | --- | --- | --- | --- |
| `tools/governance-validator/validate.mjs` | `headingTexts`, `sectionName`, and `validateRequirementLevels` | N/A — stable symbols are sufficient | Recognizes level-two record headings, removes the optional requirement-level suffix, and emits the required-level diagnostic. | `536cf5b` |
| `tools/governance-validator/validate.mjs` | `recordPathFromValue` and `validateSupersession` | N/A — stable symbols are sufficient | Locates referenced ADR/ADD/OCR paths and preserves supersession validation with shared bounded filename classification. | `536cf5b`, `31328ab` |
| `tools/governance-validator/validate.mjs` | `checklistItems` and `normalizeDimension` | N/A — stable symbols are sufficient | Supplies accepted-record and terminal-record validation with checklist rows and canonical risk-dimension names. | `536cf5b` |
| `tools/governance-validator/validate.mjs` | `validate` > record filename dispatch | N/A — stable symbols are sufficient | Routes ADR, OCR, and ADD records to their required-section and lifecycle validators. | `536cf5b` |
| `tools/governance-validator/test/validate-structure.test.mjs` and `tools/governance-validator/test/validate-lifecycle.test.mjs` | focused structural, lifecycle, and classification regression tests | N/A — stable test anchors implement the contract before or with the production change. | Exercises observable validation outcomes without asserting source text or timing. | `536cf5b`, `f9d08d5` |

**Migration and rollback strategy [Conditionally Required — this replaces or changes existing behavior]**: Replace only the nine reported implementation details. Stop if a focused contract check, the package suite, governance validation, the 900-line exception cap, or the ADR-0013 prerequisite fails. Roll back with a Git revert of this ADR's source commit; no data, runtime, or external-system migration is involved. The separately accepted OCR must not submit an analysis unless its input includes `f2cc310` or its exact equivalent.

### Engineering Exceptions [Conditionally Required — an engineering rule is exceeded or waived]

| Rule and affected unit | Measured value or condition | Rationale | Risks | Compensating controls | Owner | Removal, review, or permanent rationale | Verification evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `docs/development/software-engineering-standard.md` production-source exception limit; `tools/governance-validator/validate.mjs` | 847 physical lines at source revision `9a38dc2`, exceeding the 800-line exception limit. This ADR caps the post-change file at 900 physical lines. | The nine analyzer findings occur in the existing top-level structural validator. A module decomposition would create a distinct, broader deliverable and is explicitly excluded from this reliability-only task. | Continued high cognitive load and future coupling in the top-level validator. | Limit changes to the listed symbols; add focused real-validator tests; run `npm test`, `npm run validate`, `git diff --check`, and `wc -l`; add no dependency or public export. | @linhai | A dedicated decomposition ADR must review or remove this exception before any unrelated future modification to `validate.mjs`; this ADR may not raise the file above 900 lines. | Final source revision records the file count at or below 900, the bounded scope diff, focused tests, package suite, and governance validation. |

## Contract-To-Check Traceability [Conditionally Required — source or configuration implementation]

| Clause ID | Authoritative contract path and heading | Exact normative requirement | Acceptance check or deterministic test IDs | Explicit coverage method |
| --- | --- | --- | --- | --- |
| VSP-1 | `tools/governance-validator/validate.mjs` — `headingTexts`, `sectionName`, and `validateRequirementLevels` | Every active level-two section heading MUST have exactly one recognized requirement-level label; missing labels MUST yield the canonical `section <name> must declare a requirement level` diagnostic, while valid labels retain their normalized section name. | AC-1, AC-4 | Focused valid and invalid record fixtures exercise heading extraction, suffix normalization, and the exact diagnostic; the package suite covers all records. |
| VSP-2 | `tools/governance-validator/validate.mjs` — `validateSupersession` | A Superseded record MUST name an indexed ADR, ADD, or OCR replacement path, whose status and reciprocal `Supersedes` path satisfy the existing supersession checks. | AC-2, AC-4 | A focused lifecycle fixture exercises valid and invalid replacement-path forms and asserts the existing rejection outcome. |
| VSP-3 | `tools/governance-validator/lib/accepted-records.mjs` — `createAcceptedRecordValidator` and `tools/governance-validator/lib/terminal-validation.mjs` — `createTerminalValidator` | Active checklist rows and the five canonical risk dimensions MUST be recognized consistently by their supplied `checklistItems` and `normalizeDimension` dependencies. | AC-2, AC-4 | Focused lifecycle fixtures assert required checklist and hyphen/space-insensitive risk-dimension outcomes through the real validator. |
| VSP-4 | `tools/governance-validator/validate.mjs` — `validate` record filename dispatch | ADR, OCR, and ADD filenames MUST select their respective required-section, lifecycle, and relationship validators; non-record Markdown MUST not enter those paths. | AC-3, AC-4 | Focused structural fixtures exercise one record of each supported kind and a non-record Markdown file. |

## Risk Coverage Matrix [Conditionally Required — source or configuration implementation]

| Risk dimension | Applicability and scenario, or specific N/A reason | Owning boundary | Deterministic verification method | Exact expected result | Acceptance check IDs | Status | Actual evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| concurrency and ordering | N/A — validation runs synchronously over one repository traversal and the replacements introduce no shared mutable state or ordering protocol. | `validate` structural parsing | Structured source review and AC-4. | No shared mutable state, promise, queue, or ordering branch is introduced. | AC-4 | N/A — no concurrent behavior | N/A — `536cf5b` adds no shared state or ordering path. |
| timeout and deadline | Applicable — repository-controlled Markdown and metadata can contain long malformed structural candidates. | bounded parsing paths in `validate.mjs` | AC-1 through AC-4 and scoped analyzer verification. | Focused and package checks exit 0; the local analyzer reports zero scoped regex Reliability rows. | AC-1, AC-2, AC-3, AC-4, AC-5 | In Progress | AC-1 through AC-4 pass. OCR-0011 found one residual L584 scoped regex row for `f9d08d5`, restored the `f2cc310` baseline, and requires a narrow source follow-up before rechecking AC-5. |
| cancellation and interruption | N/A — the validator CLI has no cancellation protocol and the replacements add none. | Governance-validator CLI | Structured source review and AC-4. | No cancellation interface or lifecycle is introduced. | AC-4 | N/A — no cancellation protocol | N/A — `536cf5b` introduces no cancellation interface. |
| resource bounds and backpressure | Applicable — structural parsing must avoid unbounded regular-expression backtracking while processing repository input. | `validate.mjs` structural parsing | AC-1 through AC-4 and source review of each listed touchpoint. | The listed routines use bounded line/delimiter/literal operations; all focused and package checks exit 0. | AC-1, AC-2, AC-3, AC-4 | Pass | `536cf5b` replaces the scoped matchers with line/delimiter parsing and `replaceAll`; AC-1 through AC-4 pass. |
| framework or trust-boundary rejection | Applicable — untrusted repository Markdown and filenames must retain the existing rejection behavior for malformed record structure. | `validate` and its injected accepted/terminal validators | AC-1 through AC-4. | Focused fixtures retain each cited canonical diagnostic or success state. | AC-1, AC-2, AC-3, AC-4 | Pass | Focused tests preserve the structural, supersession, checklist, risk, and record-classification outcomes. |

## Acceptance Checks [Required]

| Check ID | Subtask | Binary acceptance point | Preconditions or input | Verification method | Exact expected result | Expected evidence | Status | Actual result and evidence |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| AC-1 | T-1 | The real validator preserves recognized requirement-level headings and rejects an active level-two heading without a requirement-level label. | New focused fixture containing valid Required/Optional/Conditionally Required headings and one unlabeled active heading. | `node --test --test-name-pattern "structural parsing contracts" test/validate-structure.test.mjs` in `tools/governance-validator`. | Process exits 0; exactly one selected test passes; its valid fixture exits 0 and its invalid fixture exits 1 with `must declare a requirement level`. | Focused Node test report. | Pass | 2026-09-01: one selected test passed (0 failures). |
| AC-2 | T-2 | The real validator retains supersession, checklist, and risk-dimension outcomes after bounded/literal replacements. | New focused lifecycle fixtures for valid and invalid supersession paths, required checklist items, and canonical risk dimensions with hyphen/space variation. | `node --test --test-name-pattern "supersession checklist and risk contracts" test/validate-lifecycle.test.mjs` in `tools/governance-validator`. | Process exits 0; exactly one selected test passes; every valid fixture exits 0 and every invalid fixture exits 1 with its established diagnostic. | Focused Node test report. | Pass | 2026-09-01: one selected test passed (0 failures) after `31328ab`, covering checklist, path, and normalized terminal-risk fixtures. |
| AC-3 | T-3 | The real validator dispatches ADR, OCR, and ADD records correctly and ignores non-record Markdown. | New focused repository fixture containing one valid record of each kind and one ordinary Markdown file. | `node --test --test-name-pattern "record classification contracts" test/validate-structure.test.mjs` in `tools/governance-validator`. | Process exits 0; exactly one selected test passes; the three records are validated and ordinary Markdown creates no record-validation error. | Focused Node test report. | Pass | 2026-09-01: one selected test passed (0 failures). |
| AC-4 | T-3 | The complete governance-validator suite, governance validation, diff check, and exception cap preserve repository contracts. | Approved source and test changes in the isolated task branch. | Run `npm test`, `npm run validate`, `git diff --check`, and `wc -l validate.mjs` in `tools/governance-validator` (use repository root for `git diff --check`). | Both npm commands exit 0, validation reports `Governance validation passed.`, `git diff --check` exits 0, and `validate.mjs` reports an integer no greater than 900. | Complete package, governance, diff, and line-count outputs. | Pass | 2026-09-01 after `31328ab`: 151/151 package tests passed; governance validation and diff check passed; `validate.mjs` is 900 lines. |
| AC-5 | T-3 | A separate accepted OCR confirms that all nine scoped local analyzer rows are absent. | AC-1 through AC-4 passed; the source revision is committed; and the OCR input includes `f2cc310` or its exact ADR-0013-equivalent source correction. | Run the OCR-defined local scanner command and inspect the overall Open/Confirmed Reliability view after processing completes. | Zero active Reliability rows remain for `validate.mjs` at the eight scoped parser locations and the literal replacement location. | OCR task identifier and issue-view evidence without credentials. | Fail | OCR-0011 processed `f9d08d5` at 10:52 and found one remaining scoped Reliability row at `validate.mjs` L584. Its successful recovery restored `f2cc310`; source correction and a new accepted OCR remain required. |

## Completion Checklist [Required]

| ID | Item | Completion Criterion | Expected Evidence | Status | Actual Evidence |
| --- | --- | --- | --- | --- | --- |
| A-1 | ADR approved | An eligible non-author approver, approval time, and exact `Approval Evidence: Approve` are recorded. | ADR metadata | Complete | @linhai approved at 2026-09-01T10:16:30+08:00 with approval evidence `Approve`. |
| A-2 | Complete task delivered | T-1 through T-3 have implementation evidence and AC-1 through AC-5 are Pass. | Implementation Plan and Acceptance Checks | In Progress | T-1 and T-2 are Complete; AC-1 through AC-4 Pass; OCR-0011 recorded AC-5 Fail for one residual L584 row and restored the baseline. |
| A-3 | Reciprocal ADD link synchronized, when applicable | N/A — this task is not derived from product demand and has no ADD candidate. | Metadata Architecture Source | N/A — no ADD applies | N/A — no product-demand ADD applies. |
| A-4 | Requirement levels satisfied | Every required section is complete, and every conditional trigger is completed or has a specific N/A reason. | Structured document review | In Progress | Terminal review awaits a successful replacement of the failed AC-5 scan evidence. |
| A-5 | Acceptance checks are decidable | Every check has one subtask, input, deterministic method, exact expected result, and evidence. | Acceptance Checks table | Complete | Accepted checks have one subtask, deterministic command, expected result, and captured AC-1 through AC-4 evidence. |
| A-6 | Engineering exceptions governed, when applicable | The `validate.mjs` exception has a complete rule, condition, rationale, controls, owner, lifecycle, and verification evidence. | Engineering Exceptions section | Complete | `31328ab` remains within the approved 900-line cap (measured 900 lines). |
| A-7 | Contract and baseline risks covered, when applicable | VSP-1 through VSP-4 map to checks, and every applicable risk reaches Pass before completion. | Traceability, matrix, and command reports | In Progress | VSP-1 through VSP-4 have AC-1 through AC-4 evidence; the residual L584 analyzer row leaves timeout/scanner evidence incomplete. |
| A-8 | Governance validation passed | The independent validator reports no document or repository validation error. | `npm run validate --prefix tools/governance-validator` output | Complete | 2026-09-01: `npm run validate` passed after the source and test changes. |

## Supporting Notes [Optional]

ADR-0013's source change `f2cc310` is now reachable from local `dev` through its terminal integration commit `1385fa3`; this branch incorporates that `dev` state in `c8bb4a7` before source verification. The OCR input therefore satisfies the ADR-0013 prerequisite when it includes this branch's source commits `536cf5b` and `f9d08d5`.

The remaining SonarQube finding was a static report on the `validateSupersession` record-type expression rather than an observable contract failure. The real-validator lifecycle test already exercises that contract; a source-text assertion or artificial runtime threshold is prohibited, so the existing contract test was rerun against `31328ab` instead of adding a non-behavioral test.

## Archival [Conditionally Required — Decision Status is `Rejected`, or Decision Status is `Deprecated` or `Superseded` and Implementation Status is final]

The record is Accepted and In Progress, so archival is inactive future-lifecycle guidance. If a qualifying retirement occurs, move this file under `docs/adr/archive/`, update its index row and every marker or reciprocal reference in the same change, retain `Superseded By: None` unless a replacement is identified, and confirm no active reference remains to the pre-archive path.

## Change Log [Required]

| Date | Change | Author |
| --- | --- | --- |
| 2026-09-01 | Drafted the isolated `validate.mjs` Reliability remediation for the nine scoped local SonarQube findings and recorded the pre-existing file-size exception and ADR-0013 scan prerequisite. | @codex |
| 2026-09-01 | Accepted by @linhai with approval evidence `Approve` at 2026-09-01T10:16:30+08:00. | @linhai |
| 2026-09-01 | Implemented bounded parsing and literal normalization in `536cf5b`, added normalized terminal-risk coverage in `f9d08d5`, and recorded AC-1 through AC-4 evidence; AC-5 remains an OCR-gated operational check. | @codex |
| 2026-09-01 | OCR-0011 processed `f9d08d5`, found one residual scoped Reliability row at `validate.mjs` L584, and restored the `f2cc310` baseline; ADR-0014 remains in progress pending a narrow follow-up correction and separately accepted recheck. | @codex |
| 2026-09-01 | Replaced the residual supersession record-type expression with shared bounded filename classification in `31328ab`; AC-2 and AC-4 passed, and a replacement OCR remains required for AC-5. | @codex |
