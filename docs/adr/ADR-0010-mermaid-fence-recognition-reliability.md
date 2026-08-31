# ADR-0010: Mermaid Fence Recognition Reliability

## Metadata [Required]

- **Decision Status**: Accepted
- **Implementation Status**: In Progress
- **Date**: 2026-08-31
- **Author**: @codex
- **Decision Owner**: @linhai
- **Required Approver**: @linhai
- **Record Scope**: Project
- **Approver [Conditionally Required — Decision Status is or has been `Accepted`]**: @linhai
- **Approval Time [Conditionally Required — Decision Status is or has been `Accepted`]**: 2026-08-31T22:18:13+08:00
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
- **Related [Optional]**: Local SonarQube overall Reliability issue list for project `koduck`, observed 2026-08-31.
- **Architecture Source [Conditionally Required — product demand]**: N/A — corrective governance-validator work requested from the local SonarQube result, not derived from product demand
- **Supersedes [Conditionally Required — this ADR replaces another]**: N/A — no ADR is replaced
- **Superseded By [Conditionally Required — this ADR is replaced]**: None

## Requirement Level Legend [Required]

- **`[Required]`**: The section or field always applies and MUST remain present with complete, verifiable content. Use `None — <reason>` only when the template explicitly permits an empty result; never leave it blank.
- **`[Conditionally Required — <trigger>]`**: The section or field MUST be completed when its stated trigger applies. When the trigger does not apply, retain `N/A — <reason>` unless the template explicitly instructs removal or retention as inactive future-lifecycle guidance. A missing trigger assessment is incomplete content.
- **`[Optional]`**: The section may be removed without affecting acceptance, execution, completion, or verification. If retained, it MUST be accurate and complete; optional content MUST NOT substitute for required evidence.

Unlabeled fields inside a `[Required]` section are required.

## Context And Problem Statement [Required]

The local SonarQube overall Reliability view reports one Open Medium finding at `tools/governance-validator/lib/mermaid-validation.mjs` line 34. Its Markdown-fence recognizer combines an unbounded backtick-or-tilde marker expression with a trailing wildcard, which the analyzer identifies as potentially super-linear for untrusted document lines.

`mermaidBlocks` is responsible for recognizing real Mermaid fences while excluding nested examples enclosed by a longer outer fence. The detected defect is static-analysis-only: the established real-validator regression already exercises the observable nested-fence behavior, so the SonarQube finding is the red baseline. A new behavior test would honestly pass before the source correction and would not prove a missing contract; source-text or timing assertions would be brittle and are prohibited.

## Scope [Required]

In scope:

- Replace only the reported opening-fence recognition expression in `mermaidBlocks` with a bounded, delimiter-aware operation that preserves accepted Markdown-fence semantics.
- Preserve recognition of backtick and tilde markers, up to three leading spaces, exact `mermaid` info strings, and nested-longer-fence exclusion.
- Verify the existing real-validator nested-fence regression and complete package suite, then verify the one target finding through a separately accepted local SonarQube OCR.

Out of scope:

- Changing Mermaid syntax parsing, ADD lifecycle rules, table-ID coverage, diagnostics, CLI behavior, SonarQube configuration, profiles, issue workflow, or credentials.
- Changing `metadata-validation.mjs`, `relationship-validation.mjs`, or `validate.mjs`, which own the remaining 14 overall Reliability findings.
- Submitting a SonarQube analysis; that operational step requires a separate accepted OCR after source verification.

## Tensions, Constraints, And Open Questions [Required]

### Identified Tensions [Conditionally Required — competing goals or trade-offs exist]

| ID | Tension | Impact | Decision |
| --- | --- | --- | --- |
| TN-1 | Fence recognition must be bounded without treating an embedded Mermaid example as a real diagram. | A broad parser rewrite can incorrectly alter ADD completeness and syntax-validation behavior; leaving the expression retains the analyzer finding. | Replace only the reported opening-marker recognition with an explicit bounded delimiter check and keep real-validator fence behavior as the semantic guard. |

### Constraints [Required]

- This is a Full ADR because the validator enforces repository-wide governance-record contracts and consumes repository-controlled Markdown.
- No dependency, public contract, record schema, scanner configuration, or issue state change is permitted.
- The analyzer finding is the red static-analysis baseline; no source-text assertion, artificial timing threshold, or test that already passes before correction may be presented as a failing behavioral regression.
- The modified governed JavaScript file must cite this ADR at its first legal comment position.

### Open Questions [Conditionally Required — material questions exist or were resolved during drafting]

None — the target line, owning validator boundary, and existing nested-fence contract identify one complete implementation slice.

## Decision Drivers [Required]

1. **Remove the reported finding**: authorized reanalysis must have no active Reliability finding at the target Mermaid-fence location.
2. **Preserve fence semantics**: real Mermaid blocks remain discoverable, while nested examples remain inert.
3. **Keep ownership narrow**: no unrelated remaining Reliability owner enters this decision.

## Options Considered [Required]

### Option: Bounded delimiter-aware opening-fence recognition

Inspect the limited indentation and first marker character, then determine marker length without a trailing wildcard expression.

Pros:

- Removes the reported expression while keeping the fence state machine local.
- Makes marker boundaries explicit and independent of regular-expression backtracking.

Cons:

- Requires careful preservation of the existing marker and info-string rules.

### Option: Suppress or relabel the finding

Leave the expression unchanged and alter analyzer state or configuration.

Pros:

- Avoids a source edit.

Cons:

- Retains the analyzer-reported unbounded recognition form.
- Changes analyzer workflow outside this implementation boundary.

### Option: Combine every remaining Reliability issue

Modify all four remaining validator modules together.

Pros:

- Could reduce the aggregate issue count in one iteration.

Cons:

- Mixes independently reviewable implementation boundaries and violates ADR scope rules.

## Decision [Required]

**Selected option**: Bounded delimiter-aware opening-fence recognition.

**Rationale**: The existing state machine already owns fence semantics. A local delimiter-aware operation removes the analyzer-reported form while the real validator continues to guard the observable nested-fence contract.

### Consequences [Required]

Positive:

- The opening-fence recognizer no longer uses the analyzer-reported potentially super-linear expression.
- Existing Markdown-fence behavior remains guarded by the real validator.

Negative:

- Fourteen overall Reliability findings remain for separately scoped decisions.

Mitigations:

- Touch only `mermaidBlocks`, run focused and full deterministic checks, and use a separately accepted OCR for analyzer verification.

## Implementation Plan [Required]

**Complete task outcome**: `mermaidBlocks` replaces the reported opening-fence expression with a bounded delimiter-aware recognition step while preserving real Mermaid-block and nested-example behavior; after separately authorized reanalysis, no active Reliability finding remains at the target location.

**Primary implementation boundary**: `tools/governance-validator/lib/mermaid-validation.mjs` > `createMermaidValidator` > `mermaidBlocks` Markdown-fence recognition.

Allowed subtask statuses: `Not Started`, `In Progress`, `Blocked`, `Complete`, or `N/A — <specific reason>`.

| ID | Objective or deliverable | Included scope | Status | Actual implementation evidence |
| --- | --- | --- | --- | --- |
| T-1 | Replace the reported opening-fence recognition with a bounded delimiter-aware operation and update the governed-file marker. | `createMermaidValidator` > `mermaidBlocks` in `tools/governance-validator/lib/mermaid-validation.mjs`. | Complete | `4d9c274` replaces the opening marker-and-tail expression with `openingFenceMarker`, preserves the governed-file ADR marker, and keeps the existing fence state machine. |
| T-2 | Verify the established nested-fence contract, complete package suite, and governance documentation contracts. | Existing Node.js tests and `npm` scripts in `tools/governance-validator`. | Complete | Focused nested-fence behavior test passed before and after the correction; `npm test` passed 146/146; `npm run validate` reported `Governance validation passed.` |

**Affected paths**: `tools/governance-validator/lib/mermaid-validation.mjs`; `tools/governance-validator/test/validation-boundary-regressions.test.mjs`; `docs/adr/ADR-0010-mermaid-fence-recognition-reliability.md`; `docs/adr/INDEX.md`.

### Stable Implementation Touchpoints [Conditionally Required — source or configuration implementation]

| Path | Stable symbol or contract anchor | Key code excerpt, when needed | Purpose | Source revision |
| --- | --- | --- | --- | --- |
| `tools/governance-validator/lib/mermaid-validation.mjs` | `createMermaidValidator` > `mermaidBlocks` | N/A — stable symbol is sufficient | Identifies real fenced Mermaid blocks and excludes a shorter example nested in a longer outer fence. | `4d9c274` |
| `tools/governance-validator/test/validation-boundary-regressions.test.mjs` | `does not syntax-check a Mermaid example nested inside an outer fence` | N/A — stable test anchor is sufficient | Exercises the real validator’s nested-fence exclusion contract. | `27a13d1` |

**Migration and rollback strategy [Conditionally Required — this replaces or changes existing behavior]**: Replace only the opening-fence recognition operation; stop if the focused regression or complete package suite fails. Rollback is a Git revert of the implementation commit, restoring the prior expression; no data or runtime migration is involved.

### Engineering Exceptions [Conditionally Required — an engineering rule is exceeded or waived]

N/A — the localized replacement does not exceed or waive a software-engineering rule.

## Contract-To-Check Traceability [Conditionally Required — source or configuration implementation]

| Clause ID | Authoritative contract path and heading | Exact normative requirement | Acceptance check or deterministic test IDs | Explicit coverage method |
| --- | --- | --- | --- | --- |
| TC-1 | `tools/governance-validator/lib/mermaid-validation.mjs` — `mermaidBlocks` | Only a real same-character Markdown fence with an exact `mermaid` info string yields a Mermaid block. | AC-1, AC-2 | The real validator runs the nested-fence fixture and the complete package suite. |
| TC-2 | `tools/governance-validator/lib/mermaid-validation.mjs` — `validateMermaid` | Mermaid-looking content nested inside a longer outer fence must not be syntax-checked or satisfy a diagram gate. | AC-1, AC-2 | The focused real-validator regression asserts a successful result for the nested invalid example; the complete suite covers adjacent completeness gates. |

## Risk Coverage Matrix [Conditionally Required — source or configuration implementation]

| Risk dimension | Applicability and scenario, or specific N/A reason | Owning boundary | Deterministic verification method | Exact expected result | Acceptance check IDs | Status | Actual evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| concurrency and ordering | N/A — fence recognition processes one document synchronously and the replacement adds no shared state. | `mermaidBlocks` | Structured source review and AC-2. | No shared mutable state or ordering contract is introduced. | AC-2 | N/A — no concurrent behavior | Source review of `4d9c274` found only local variables and ordered line iteration; AC-2 passed. |
| timeout and deadline | Applicable — a repository Markdown line may contain arbitrarily long fence-marker text. | `mermaidBlocks` | AC-2 package suite and separately authorized analyzer reanalysis. | Package suite exits 0, and reanalysis has no active finding at the reported opening-fence location. | AC-2, AC-4 | In Progress | `npm test` passed 146/146; analyzer reanalysis remains pending separately accepted OCR. |
| cancellation and interruption | N/A — the validator CLI has no cancellation protocol and this change adds none. | Governance-validator CLI | Structured source review and AC-2. | No cancellation interface or lifecycle is introduced. | AC-2 | N/A — no cancellation protocol | Source review of `4d9c274` confirms no cancellation interface or lifecycle was added; AC-2 passed. |
| resource bounds and backpressure | Applicable — Markdown documents can contain arbitrary fence lines and nested examples. | `mermaidBlocks` | AC-1 and AC-2. | Focused and complete real-validator checks exit 0 while retaining nested-example exclusion. | AC-1, AC-2 | Pass | Focused real-validator regression passed after the correction; `npm test` passed 146/146. |
| framework or trust-boundary rejection | Applicable — repository Markdown is validator input that must retain real-block recognition and invalid-example exclusion. | `createMermaidValidator` | AC-1 focused real-validator test. | Focused test exits 0 and the nested invalid example remains unparsed. | AC-1 | Pass | Focused nested-fence real-validator regression passed after the correction. |

## Acceptance Checks [Required]

| Check ID | Subtask | Binary acceptance point | Preconditions or input | Verification method | Exact expected result | Expected evidence | Status | Actual result and evidence |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| AC-1 | T-1 | The real validator does not syntax-check a Mermaid-looking invalid example nested in a longer outer fence. | Existing `does not syntax-check a Mermaid example nested inside an outer fence` fixture. | `node --test --test-name-pattern "does not syntax-check a Mermaid example nested inside an outer fence" test/validation-boundary-regressions.test.mjs` in `tools/governance-validator`. | Process exits 0 with exactly one passing selected test and no failed selected test. | Focused Node test report. | Pass | Passed before the correction (327.895 ms) and after `4d9c274` (335.790 ms): exactly one selected test passed and none failed. |
| AC-2 | T-2 | The complete governance-validator suite preserves existing Mermaid and governance-record contracts. | Accepted implementation in the isolated task branch. | `npm test` in `tools/governance-validator`. | Process exits 0 with zero failed tests. | Full package test report. | Pass | After `4d9c274`, `npm test` exited 0: 146 tests passed, 0 failed (18.95 s). |
| AC-3 | T-2 | Repository governance validation accepts the ADR/index state and unchanged record contracts. | All task changes present in the isolated task branch. | `npm run validate` in `tools/governance-validator`. | Process exits 0 and reports `Governance validation passed.` | Governance-validation command output. | Pass | After `4d9c274`, `npm run validate` exited 0 and reported `Governance validation passed.` |
| AC-4 | T-2 | A separate accepted OCR verifies the analyzer outcome. | Source correction, AC-1 through AC-3 passed, and an accepted local SonarQube verification OCR. | The specified local Reliability issue view after the OCR analysis completes. | Zero active Reliability findings remain at the former `mermaid-validation.mjs` target location. | OCR task and issue-view evidence without credentials. | Not Started | Not run — separate OCR not yet proposed. |

## Completion Checklist [Required]

| ID | Item | Completion Criterion | Expected Evidence | Status | Actual Evidence |
| --- | --- | --- | --- | --- | --- |
| A-1 | ADR approved | An eligible non-author approver, approval time, and exact `Approval Evidence: Approve` are recorded. | ADR metadata | Complete | @linhai approved at 2026-08-31T22:18:13+08:00 with `Approval Evidence: Approve`. |
| A-2 | Complete task delivered | T-1 through T-2 have implementation evidence and AC-1 through AC-4 are Pass. | Implementation Plan and Acceptance Checks | Not Started | Not run — awaiting implementation and OCR verification. |
| A-3 | Reciprocal ADD link synchronized, when applicable | N/A — the task is not derived from product demand and has no ADD candidate. | Metadata Architecture Source | N/A — no ADD applies | N/A — no product-demand ADD applies. |
| A-4 | Requirement levels satisfied | Every required section is complete, and every conditional trigger is completed or has a specific N/A reason. | Structured document review | Not Started | Not run — terminal review follows implementation. |
| A-5 | Acceptance checks are decidable | Every check has one subtask, input, deterministic method, exact expected result, and evidence. | Acceptance Checks table | Not Started | Not run — terminal review follows implementation. |
| A-6 | Engineering exceptions governed, when applicable | N/A — no engineering exception is planned. | Engineering Exceptions section | N/A — no exception applies | N/A — no engineering rule is exceeded or waived. |
| A-7 | Contract and baseline risks covered, when applicable | TC-1 through TC-2 map to checks, and every applicable risk reaches Pass before completion. | Traceability, matrix, and command reports | Not Started | Not run — awaiting implementation and analyzer verification. |
| A-8 | Governance validation passed | The independent validator reports no document or repository validation error. | `npm run validate` output | Complete | After `4d9c274`, `npm run validate` exited 0 and reported `Governance validation passed.` |

## Supporting Notes [Optional]

The other 14 overall Reliability findings are explicitly deferred so this Mermaid-fence slice remains independently reviewable. The overall count can remain nonzero after this slice; AC-4 concerns only the target `mermaid-validation.mjs` location. The static analyzer finding supplied the red baseline: the existing real-validator test already passed before the correction, so it is retained as contract evidence rather than misrepresented as a newly failing behavioral test.

## Archival [Conditionally Required — Decision Status is `Rejected`, or Decision Status is `Deprecated` or `Superseded` and Implementation Status is final]

The record is Proposed and not archival-eligible. If a later rejection, deprecation, or supersession triggers archival, move it under `docs/adr/archive/`, update all governed-file markers and references in the same change, and update its single index row.

## Change Log [Required]

| Date | Change | Author |
| --- | --- | --- |
| 2026-08-31 | Drafted the Full ADR for the one overall SonarQube Reliability finding in Mermaid-fence recognition. | @codex |
| 2026-08-31 | Accepted by @linhai with approval evidence `Approve` at 2026-08-31T22:18:13+08:00. | @codex |
| 2026-08-31 | Implemented the bounded opening-fence recognizer in `4d9c274`; focused and full checks passed. | @codex |
