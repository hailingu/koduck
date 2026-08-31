# ADR-0008: Delimiter-Bounded Governance Record Paths

## Metadata [Required]

- **Decision Status**: Deprecated
- **Implementation Status**: Complete
- **Date**: 2026-08-31
- **Author**: @codex
- **Decision Owner**: @linhai
- **Required Approver**: @linhai
- **Record Scope**: Project
- **Approver [Conditionally Required — Decision Status is or has been `Accepted`]**: @linhai
- **Approval Time [Conditionally Required — Decision Status is or has been `Accepted`]**: 2026-08-31T20:55:58+08:00
- **Approval Evidence [Conditionally Required — Decision Status is or has been `Accepted`]**: Approve
- **Rejector [Conditionally Required — Decision Status is `Rejected`]**: N/A — Decision Status is `Accepted`
- **Rejection Time [Conditionally Required — Decision Status is `Rejected`]**: N/A — Decision Status is `Accepted`
- **Rejection Evidence [Conditionally Required — Decision Status is `Rejected`]**: N/A — Decision Status is `Accepted`
- **Retired By [Conditionally Required — Decision Status is `Deprecated` or `Superseded`]**: @linhai
- **Retirement Time [Conditionally Required — Decision Status is `Deprecated` or `Superseded`]**: 2026-08-31T22:01:15+08:00
- **Retirement Evidence [Conditionally Required — Decision Status is `Deprecated` or `Superseded`]**: Deprecate
- **Retirement Reason [Conditionally Required — Decision Status is `Deprecated` or `Superseded`]**: The completed local Reliability remediation is retained as historical evidence; the Decision Owner directed archival before further Reliability remediation.
- **Blocked From [Conditionally Required — Implementation Status is `Blocked`]**: N/A — implementation is not blocked
- **Blocker And Evidence [Conditionally Required — Implementation Status is `Blocked`]**: N/A — implementation is not blocked
- **Blocker Owner [Conditionally Required — Implementation Status is `Blocked`]**: N/A — implementation is not blocked
- **Blocker Exit Or Recheck Criterion [Conditionally Required — Implementation Status is `Blocked`]**: N/A — implementation is not blocked
- **Related [Optional]**: Local SonarQube new-code issue list for project `koduck`, observed 2026-08-31; preceding archived `docs/adr/archive/ADR-0007-linear-time-governance-path-recognition.md`.
- **Architecture Source [Conditionally Required — product demand]**: N/A — corrective governance-validator work requested from the local SonarQube result, not derived from product demand
- **Supersedes [Conditionally Required — this ADR replaces another]**: N/A — archived ADR-0007 is a separate prior correction; this is a residual-rule correction
- **Superseded By [Conditionally Required — this ADR is replaced]**: None

## Requirement Level Legend [Required]

- **`[Required]`**: The section or field always applies and MUST remain present with complete, verifiable content. Use `None — <reason>` only when the template explicitly permits an empty result; never leave it blank.
- **`[Conditionally Required — <trigger>]`**: The section or field MUST be completed when its stated trigger applies. When the trigger does not apply, retain `N/A — <reason>` unless the template explicitly instructs removal or retention as inactive future-lifecycle guidance. A missing trigger assessment is incomplete content.
- **`[Optional]`**: The section may be removed without affecting acceptance, execution, completion, or verification. If retained, it MUST be accurate and complete; optional content MUST NOT substitute for required evidence.

Unlabeled fields inside a `[Required]` section are required.

## Context And Problem Statement [Required]

ADR-0007 replaced three nested path-recognition expressions with slash-constrained variants and the Security filter then reported zero findings. The subsequent local SonarQube analysis still reports the same three source locations as Medium Reliability findings: `validateIndex` in the relationship validator and the ADR/ADD relationship-path recognition in the CLI. The revised expressions continue to contain repeated variable-width path segments, which SonarQube identifies as potentially super-linear.

The existing real-validator adversarial regression remains green, so it must be preserved as the behavioral guard. A proposed test for a path ending in `.md.` was rejected before implementation because the current reciprocal-link validation already rejects that value; it does not represent a missing behavior. The residual defect is therefore the static analyzer finding at three expression sites, not an unproven path-acceptance change.

## Scope [Required]

In scope:

- Retain the existing real-validator adversarial regression as the behavior-preservation guard and record the current three-finding SonarQube baseline.
- Replace the remaining three pattern-based extraction sites with one delimiter-bounded, procedural governance-record path recognizer that preserves existing accepted and rejected behavior.
- Preserve valid ADR, ADD, project, and service-relative path recognition through the routed validator suites.

Out of scope:

- Altering the supported record path grammar, index schema, command-line interface, SonarQube configuration, profiles, issue state, quality gate, or credentials.
- Refactoring unrelated governance-validation rules, adding dependencies, or changing unrelated Reliability findings.
- Submitting a new SonarQube analysis; that local operation requires a separate accepted OCR after source verification.

## Tensions, Constraints, And Open Questions [Required]

### Identified Tensions [Conditionally Required — competing goals or trade-offs exist]

| ID | Tension | Impact | Decision |
| --- | --- | --- | --- |
| TN-1 | Existing accepted and rejected path behavior must remain stable while the repeated-variable-width expressions are removed. | An overly broad replacement can change record resolution; a further expression tweak can retain the analyzer finding. | Use one small procedural helper inside the existing relationship-validation boundary, retain existing file-resolution validation, and preserve the real-validator adversarial regression. |

### Constraints [Required]

- This is a Full ADR because it corrects a residual untrusted-input path-recognition concern; it must be Accepted before test or source implementation changes.
- No dependency, public contract, record schema, scanner configuration, or issue workflow change is permitted.
- The implementation must use Red-Green-Refactor with the existing Node.js test harness and add a behavioral regression before production code.
- Every modified governed JavaScript source file must cite this ADR at its first legal comment position.

### Open Questions [Conditionally Required — material questions exist or were resolved during drafting]

None — the local issue list identifies all three remaining extraction locations and the complete-token behavior is deterministically testable with the existing fixtures.

## Decision Drivers [Required]

1. **Eliminate the residual findings**: no active SonarQube Reliability finding may remain for the three former path-recognition expressions.
2. **Preserve established behavior**: the existing real-validator adversarial and valid-fixture behavior must remain unchanged.
3. **Keep the correction narrow**: valid nested repository paths continue through the existing resolver and fixture suite without a new package.

## Options Considered [Required]

### Option: Delimiter-bounded procedural path recognition

Split candidate text at the existing Markdown delimiters and accept a token only when its complete record-directory and filename form match the requested record type.

Pros:

- Removes the three repeated-variable-width path expressions reported by SonarQube.
- Keeps file containment and relationship validation in their existing owners.

Cons:

- Adds a small helper while the existing focused regression preserves the documented behavior.

### Option: Further constrain the existing expressions

Modify the character classes or repetition limits of the current expressions.

Pros:

- Keeps the current extraction form.

Cons:

- The previous constraint-only change left all three Reliability findings active.
- It cannot clearly express whole-token acceptance without further overlapping variable-width matching.

### Option: Suppress or relabel the SonarQube findings

Leave the expressions in place and change issue workflow state or analysis configuration.

Pros:

- Requires no source change.

Cons:

- Does not remove the analyzer concern or repair the demonstrated suffix-truncation behavior.
- Changes external issue state outside this implementation boundary.

## Decision [Required]

**Selected option**: Delimiter-bounded procedural path recognition.

**Rationale**: Record paths already occur in Markdown-delimited fields. Treating each complete token as the unit of recognition removes the analyzer-reported repetition while retaining the existing resolver as the single source of truth for containment and file validity; the existing real-validator regression guards the intended malformed-input behavior.

### Consequences [Required]

Positive:

- The three pattern sites no longer use variable-width repeated path expressions.
- The existing valid and malformed record-path behavior remains guarded without retaining the reported expressions.

Negative:

- The helper needs a clear, focused contract for both ADR and ADD path recognition.

Mitigations:

- Retain the existing adversarial timeout regression, run the complete package and governance checks, then verify the three SonarQube findings through a separately accepted local OCR.

## Implementation Plan [Required]

**Complete task outcome**: The governance validator replaces the three remaining expression-based record-path extraction sites with one delimiter-bounded procedural extractor while preserving the existing real-validator adversarial regression, leaving no active SonarQube Reliability finding at the three reported locations after separately authorized reanalysis.

**Primary implementation boundary**: `tools/governance-validator` governance-record path extraction for index and reciprocal relationship validation.

Allowed subtask statuses: `Not Started`, `In Progress`, `Blocked`, `Complete`, or `N/A — <specific reason>`.

| ID | Objective or deliverable | Included scope | Status | Actual implementation evidence |
| --- | --- | --- | --- | --- |
| T-1 | Establish the three-finding SonarQube Reliability baseline and retain the existing real-validator adversarial regression as the behavior-preservation guard. | Local issue evidence and `tools/governance-validator/test/index-path-validation.test.mjs`. | Complete | The local issue list showed three Medium Reliability findings at the reported locations. The existing adversarial-index test passed before the source change in about 304 ms, returning the expected validator exit status 1 without a timeout. |
| T-2 | Replace the three reported extraction sites with delimiter-bounded procedural record-path recognition and update governed-file markers. | `tools/governance-validator/lib/relationship-validation.mjs` and `tools/governance-validator/validate.mjs`. | Complete | Commit `4be9383` adds `recordPathTokens` and `findRecordPath`, replaces the three expression-based extraction sites, removes the CLI pattern context, and updates both governed-file markers to this ADR. |
| T-3 | Verify the existing adversarial regression, complete test suite, and governance validation. | Existing Node.js tests and `npm` scripts in `tools/governance-validator`. | Complete | The focused adversarial test passed after the change in about 311 ms; terminal `npm test` passed 146/146 tests; terminal `npm run validate` passed. Accepted OCR-0004 then verified the version `4be9383` local analysis and zero requested issue results. |

**Affected paths**: `tools/governance-validator/test/index-path-validation.test.mjs`; `tools/governance-validator/lib/relationship-validation.mjs`; `tools/governance-validator/validate.mjs`; `docs/adr/archive/ADR-0008-delimiter-bounded-governance-record-paths.md`; `docs/adr/INDEX.md`.

### Stable Implementation Touchpoints [Conditionally Required — source or configuration implementation]

| Path | Stable symbol or contract anchor | Key code excerpt, when needed | Purpose | Source revision |
| --- | --- | --- | --- | --- |
| `tools/governance-validator/test/index-path-validation.test.mjs` | `rejects an adversarial index-path without timing out` | N/A — stable test anchor is sufficient | Runs the production validator against a controlled malformed repository fixture. | `4be9383` |
| `tools/governance-validator/lib/relationship-validation.mjs` | `recordPathTokens`, `findRecordPath`, and `createRelationshipValidator` | N/A — stable symbols are sufficient | Owns delimiter-bounded index fallback and linked ADR/ADD record-path extraction. | `4be9383` |
| `tools/governance-validator/validate.mjs` | `relationshipValidator` construction | N/A — stable symbol is sufficient | Uses the shared relationship validator without CLI pattern context. | `4be9383` |

**Migration and rollback strategy [Conditionally Required — this replaces or changes existing behavior]**: Replace only extraction of delimiter-bounded record tokens; stop if a valid project or service-relative fixture fails. Rollback is a Git revert of the implementation commit, restoring the prior expression-based extraction; no data or runtime migration is involved.

### Engineering Exceptions [Conditionally Required — an engineering rule is exceeded or waived]

N/A — the planned localized helper and regression do not exceed or waive a software-engineering rule.

## Contract-To-Check Traceability [Conditionally Required — source or configuration implementation]

| Clause ID | Authoritative contract path and heading | Exact normative requirement | Acceptance check or deterministic test IDs | Explicit coverage method |
| --- | --- | --- | --- | --- |
| TC-1 | `docs/adr/archive/ADR-0008-delimiter-bounded-governance-record-paths.md` — Decision | Delimiter-bounded record-path extraction must preserve the existing malformed-input rejection behavior without expression backtracking. | AC-1 | The real CLI processes the existing adversarial index fixture with no timeout error and rejects it with exit status 1. |
| TC-2 | `tools/governance-validator/README.md` — Governance Validator | The validator enforces deterministic ADD, ADR, and OCR structure and lifecycle contracts. | AC-2, AC-3 | The focused regressions, complete package suite, and repository governance command complete with their exact expected results. |

## Risk Coverage Matrix [Conditionally Required — source or configuration implementation]

| Risk dimension | Applicability and scenario, or specific N/A reason | Owning boundary | Deterministic verification method | Exact expected result | Acceptance check IDs | Status | Actual evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| concurrency and ordering | N/A — the validator performs synchronous parsing with no shared mutable concurrent state. | Governance-validator CLI | Structured source review and AC-2. | No concurrent state or ordering contract is introduced. | AC-2 | N/A — no concurrent behavior | Source review confirms the helper processes one string synchronously and introduces no shared state. |
| timeout and deadline | Applicable — the three current expressions are reported as potentially super-linear on malformed path text. | Governance record-path extraction | Existing adversarial timeout regression plus AC-2. | The existing adversarial fixture has no timeout error and the focused suite exits zero. | AC-2 | Pass | The focused adversarial test passed before and after the change; terminal `npm test` passed 146/146 tests with no timeout error. |
| cancellation and interruption | N/A — the CLI exposes no cancellation protocol and the correction does not add one. | Governance-validator CLI | Structured source review and AC-2. | No cancellation interface or lifecycle is introduced. | AC-2 | N/A — no cancellation protocol | Source review confirms the helper adds no cancellation interface or lifecycle. |
| resource bounds and backpressure | Applicable — delimiter tokenization must process malformed input without regex backtracking. | Governance record-path extraction | Existing adversarial timeout regression and AC-2. | The adversarial fixture completes within the existing 1,000 ms child-process timeout. | AC-2 | Pass | The adversarial test completed in about 311 ms after the change, within its existing 1,000 ms timeout. |
| framework or trust-boundary rejection | Applicable — candidate links and Markdown index cells are untrusted repository inputs. | `createRelationshipValidator` relationship extraction | Existing real-validator adversarial regression. | The malformed reference fails validation with exit status 1 and no timeout error. | AC-1 | Pass | The focused test retained the expected validator exit status 1 and no `ETIMEDOUT` error after the helper replacement. |

## Acceptance Checks [Required]

| Check ID | Subtask | Binary acceptance point | Preconditions or input | Verification method | Exact expected result | Expected evidence | Status | Actual result and evidence |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| AC-1 | T-1 | The actual validator rejects the existing adversarial index-path fixture without a child-process timeout. | A temporary valid repository with one index row containing `segment/` repeated 24 times followed by `docs` and no valid record path. | `node --test --test-name-pattern "adversarial index-path" test/index-path-validation.test.mjs` in `tools/governance-validator`. | Exit status 0; the selected test passes; its spawned validator result has exit status 1 and no `ETIMEDOUT` error. | Focused Node test report. | Pass | Before and after the source change, the selected test passed in about 304 ms and 311 ms respectively; its spawned validator retained exit status 1 with no timeout. |
| AC-2 | T-2 | The package regressions and complete test suite pass after replacing the three expression sites and governed-file markers. | Accepted implementation in the isolated task branch. | `npm test` in `tools/governance-validator`. | Exit status 0 with no failing test; the existing adversarial index-path regression has no timeout error. | Full package test report. | Pass | Terminal `npm test` exited 0 with 146/146 tests passing after commit `4be9383`; no timeout error occurred. |
| AC-3 | T-3 | Repository governance validation accepts the new ADR/index state and unchanged record contracts. | All task changes present in the isolated task branch. | `npm run validate` in `tools/governance-validator`. | Exit status 0 and no validation errors. | Governance-validation command output. | Pass | Terminal `npm run validate` exited 0 with no validation errors against the completed ADR and archived OCR state. |

## Completion Checklist [Required]

| ID | Item | Completion Criterion | Expected Evidence | Status | Actual Evidence |
| --- | --- | --- | --- | --- | --- |
| A-1 | ADR approved | An eligible non-author approver, approval time, and exact `Approval Evidence: Approve` are recorded. | ADR metadata | Complete | @linhai approved the revised ADR at 2026-08-31T20:55:58+08:00 with `Approval Evidence: Approve`. |
| A-2 | Complete task delivered | T-1 through T-3 have implementation evidence and AC-1 through AC-3 are Pass. | Implementation Plan and Acceptance Checks | Complete | Commit `4be9383` and archived OCR-0004 record the correction, successful local analysis, and all acceptance outcomes. |
| A-3 | Reciprocal ADD link synchronized, when applicable | N/A — the task is not derived from product demand and has no ADD candidate. | Metadata Architecture Source | N/A — no ADD applies | N/A — no product-demand ADD applies. |
| A-4 | Requirement levels satisfied | Every required section is complete, and every conditional trigger is completed or has a specific N/A reason. | Structured document review | Complete | Terminal ADR and OCR review records complete required content and specific non-trigger recovery evidence. |
| A-5 | Acceptance checks are decidable | Every check has one subtask, input, deterministic method, exact expected result, and evidence. | Acceptance Checks table | Complete | AC-1 through AC-3 retain their declared subtask, input, deterministic method, exact expected result, and recorded evidence. |
| A-6 | Engineering exceptions governed, when applicable | N/A — no engineering exception is planned. | Engineering Exceptions section | N/A — no exception applies | N/A — no engineering rule is exceeded or waived. |
| A-7 | Contract and baseline risks covered, when applicable | TC-1 and TC-2 map to checks, and every applicable risk reaches Pass before completion. | Traceability, matrix, and command reports | Complete | TC-1 and TC-2 map to passing AC-1 through AC-3; each applicable risk row is Pass. OCR-0004 verifies the external analyzer outcome. |
| A-8 | Governance validation passed | The independent validator reports no document or repository validation error. | `npm run validate` output | Complete | Terminal `npm run validate` passed against the completed ADR and archived OCR state. |

## Supporting Notes [Optional]

The pre-correction local issue list showed exactly three Medium Reliability findings, one in `relationship-validation.mjs` and two in `validate.mjs`. After accepted OCR-0004 submitted source version `4be9383`, the requested Open/Confirmed new-code view showed zero Reliability findings and zero total issues. The rejected trailing-punctuation test is intentionally not retained because current reciprocal-link behavior already rejects that input.

## Archival [Conditionally Required — Decision Status is `Rejected`, or Decision Status is `Deprecated` or `Superseded` and Implementation Status is final]

The record is `Deprecated / Complete` and is archived under `docs/adr/archive/`; all governed-file markers, references, and its index row use the archived path.

## Change Log [Required]

| Date | Change | Author |
| --- | --- | --- |
| 2026-08-31 | Drafted the Full ADR for the residual three SonarQube Reliability findings in governance-record path extraction. | @codex |
| 2026-08-31 | Accepted by @linhai with approval evidence `Approve` at 2026-08-31T20:50:11+08:00. | @codex |
| 2026-08-31 | Approval-invalidating correction: removed the unsupported trailing-punctuation behavior claim and restored Proposed status; previous approval by @linhai at 2026-08-31T20:50:11+08:00 with evidence `Approve` remains historical only. | @codex |
| 2026-08-31 | Reaccepted by @linhai with approval evidence `Approve` at 2026-08-31T20:55:58+08:00. | @codex |
| 2026-08-31 | Implemented delimiter-bounded record path extraction in commit `4be9383`; focused regression, full package suite, and governance validation passed. | @codex |
| 2026-08-31 | Accepted OCR-0004 verified local analysis version `4be9383` with Quality Gate `Passed` and zero issues in the requested new-code view. | @codex |
| 2026-08-31 | @linhai issued `Deprecate` at 2026-08-31T22:01:15+08:00; archived the completed Reliability remediation decision with no replacement record. | @codex |
