# ADR-0008: Delimiter-Bounded Governance Record Paths

## Metadata [Required]

- **Decision Status**: Accepted
- **Implementation Status**: In Progress
- **Date**: 2026-08-31
- **Author**: @codex
- **Decision Owner**: @linhai
- **Required Approver**: @linhai
- **Record Scope**: Project
- **Approver [Conditionally Required — Decision Status is or has been `Accepted`]**: @linhai
- **Approval Time [Conditionally Required — Decision Status is or has been `Accepted`]**: 2026-08-31T20:50:11+08:00
- **Approval Evidence [Conditionally Required — Decision Status is or has been `Accepted`]**: Approve
- **Rejector [Conditionally Required — Decision Status is `Rejected`]**: N/A — Decision Status is `Accepted`
- **Rejection Time [Conditionally Required — Decision Status is `Rejected`]**: N/A — Decision Status is `Accepted`
- **Rejection Evidence [Conditionally Required — Decision Status is `Rejected`]**: N/A — Decision Status is `Accepted`
- **Retired By [Conditionally Required — Decision Status is `Deprecated` or `Superseded`]**: N/A — record is not retired
- **Retirement Time [Conditionally Required — Decision Status is `Deprecated` or `Superseded`]**: N/A — record is not retired
- **Retirement Evidence [Conditionally Required — Decision Status is `Deprecated` or `Superseded`]**: N/A — record is not retired
- **Retirement Reason [Conditionally Required — Decision Status is `Deprecated` or `Superseded`]**: N/A — record is not retired
- **Blocked From [Conditionally Required — Implementation Status is `Blocked`]**: N/A — implementation is not blocked
- **Blocker And Evidence [Conditionally Required — Implementation Status is `Blocked`]**: N/A — implementation is not blocked
- **Blocker Owner [Conditionally Required — Implementation Status is `Blocked`]**: N/A — implementation is not blocked
- **Blocker Exit Or Recheck Criterion [Conditionally Required — Implementation Status is `Blocked`]**: N/A — implementation is not blocked
- **Related [Optional]**: Local SonarQube new-code issue list for project `koduck`, observed 2026-08-31; preceding `docs/adr/ADR-0007-linear-time-governance-path-recognition.md`.
- **Architecture Source [Conditionally Required — product demand]**: N/A — corrective governance-validator work requested from the local SonarQube result, not derived from product demand
- **Supersedes [Conditionally Required — this ADR replaces another]**: N/A — ADR-0007 remains complete; this is a separate residual-rule correction
- **Superseded By [Conditionally Required — this ADR is replaced]**: None

## Requirement Level Legend [Required]

- **`[Required]`**: The section or field always applies and MUST remain present with complete, verifiable content. Use `None — <reason>` only when the template explicitly permits an empty result; never leave it blank.
- **`[Conditionally Required — <trigger>]`**: The section or field MUST be completed when its stated trigger applies. When the trigger does not apply, retain `N/A — <reason>` unless the template explicitly instructs removal or retention as inactive future-lifecycle guidance. A missing trigger assessment is incomplete content.
- **`[Optional]`**: The section may be removed without affecting acceptance, execution, completion, or verification. If retained, it MUST be accurate and complete; optional content MUST NOT substitute for required evidence.

Unlabeled fields inside a `[Required]` section are required.

## Context And Problem Statement [Required]

ADR-0007 replaced three nested path-recognition expressions with slash-constrained variants and the Security filter then reported zero findings. The subsequent local SonarQube analysis still reports the same three source locations as Medium Reliability findings: `validateIndex` in the relationship validator and the ADR/ADD relationship-path recognition in the CLI. The revised expressions continue to contain repeated variable-width path segments, which SonarQube identifies as potentially super-linear.

The current expressions also locate a path as a substring instead of requiring the complete delimiter-bounded token. For example, a selected candidate path ending in `.md.` can be reduced to its `.md` prefix and accepted. The validator must instead recognize only complete governance-record path tokens while preserving valid nested project or service paths.

## Scope [Required]

In scope:

- Add one real-validator regression that proves a selected ADR path with trailing punctuation is rejected rather than truncated to a valid record path.
- Replace the remaining three pattern-based extraction sites with one delimiter-bounded, procedural governance-record path recognizer.
- Preserve valid ADR, ADD, project, and service-relative path recognition through the routed validator suites.

Out of scope:

- Altering the supported record path grammar, index schema, command-line interface, SonarQube configuration, profiles, issue state, quality gate, or credentials.
- Refactoring unrelated governance-validation rules, adding dependencies, or changing unrelated Reliability findings.
- Submitting a new SonarQube analysis; that local operation requires a separate accepted OCR after source verification.

## Tensions, Constraints, And Open Questions [Required]

### Identified Tensions [Conditionally Required — competing goals or trade-offs exist]

| ID | Tension | Impact | Decision |
| --- | --- | --- | --- |
| TN-1 | Full-token validation must reject suffix-contaminated paths while accepted project and service-relative paths must remain recognizable. | A permissive extractor can validate a substring; an over-restrictive extractor can reject valid nested records. | Split only on existing record delimiters and accept a token only when its complete final component has the required ADR or ADD filename form. |
| TN-2 | The implementation must eliminate SonarQube's repeated-variable-width expression findings without introducing a broad parser framework. | Further expression tuning can retain the analyzer finding; a broad parser adds unrelated behavior. | Use one small procedural helper inside the existing relationship-validation boundary and retain existing file-resolution validation. |

### Constraints [Required]

- This is a Full ADR because it corrects a residual untrusted-input path-recognition concern; it must be Accepted before test or source implementation changes.
- No dependency, public contract, record schema, scanner configuration, or issue workflow change is permitted.
- The implementation must use Red-Green-Refactor with the existing Node.js test harness and add a behavioral regression before production code.
- Every modified governed JavaScript source file must cite this ADR at its first legal comment position.

### Open Questions [Conditionally Required — material questions exist or were resolved during drafting]

None — the local issue list identifies all three remaining extraction locations and the complete-token behavior is deterministically testable with the existing fixtures.

## Decision Drivers [Required]

1. **Eliminate the residual findings**: no active SonarQube Reliability finding may remain for the three former path-recognition expressions.
2. **Reject truncation**: a record reference is valid only when its entire delimiter-bounded token is a recognized ADR or ADD path.
3. **Preserve established behavior**: valid nested repository paths continue through the existing resolver and fixture suite without a new package.

## Options Considered [Required]

### Option: Delimiter-bounded procedural path recognition

Split candidate text at the existing Markdown delimiters and accept a token only when its complete record-directory and filename form match the requested record type.

Pros:

- Removes the three repeated-variable-width path expressions reported by SonarQube.
- Prevents a valid `.md` prefix from accepting a token with a trailing punctuation suffix.
- Keeps file containment and relationship validation in their existing owners.

Cons:

- Adds a small helper and focused regression to preserve the documented token grammar.

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

**Rationale**: Record paths already occur in Markdown-delimited fields. Treating each complete token as the unit of recognition removes the analyzer-reported repetition and makes suffix rejection explicit, while the existing resolver remains the single source of truth for containment and file validity.

### Consequences [Required]

Positive:

- The three pattern sites no longer use variable-width repeated path expressions.
- A record path with trailing punctuation cannot be silently truncated and accepted.

Negative:

- The helper needs a clear, focused contract for both ADR and ADD path recognition.

Mitigations:

- Test the production validator against the trailing-punctuation fixture, retain the existing adversarial timeout regression, and run the complete package and governance checks.

## Implementation Plan [Required]

**Complete task outcome**: The governance validator rejects a selected record path with a trailing punctuation suffix and recognizes only complete ADR and ADD path tokens through a shared procedural extractor, leaving no active SonarQube Reliability finding at the three reported locations after separately authorized reanalysis.

**Primary implementation boundary**: `tools/governance-validator` governance-record path extraction for index and reciprocal relationship validation.

Allowed subtask statuses: `Not Started`, `In Progress`, `Blocked`, `Complete`, or `N/A — <specific reason>`.

| ID | Objective or deliverable | Included scope | Status | Actual implementation evidence |
| --- | --- | --- | --- | --- |
| T-1 | Add a behavioral regression for a selected ADR reference whose complete token ends in `.md.`. | `tools/governance-validator/test/index-path-validation.test.mjs` and existing temporary fixture helpers. | In Progress | Red test not yet run. |
| T-2 | Replace the three reported extraction sites with delimiter-bounded procedural record-path recognition and update governed-file markers. | `tools/governance-validator/lib/relationship-validation.mjs` and `tools/governance-validator/validate.mjs`. | Not Started | Not run — awaiting T-1 red result. |
| T-3 | Verify the new regression, existing adversarial timeout regression, complete test suite, and governance validation. | Existing Node.js tests and `npm` scripts in `tools/governance-validator`. | Not Started | Not run — awaiting T-2 implementation. |

**Affected paths**: `tools/governance-validator/test/index-path-validation.test.mjs`; `tools/governance-validator/lib/relationship-validation.mjs`; `tools/governance-validator/validate.mjs`; `docs/adr/ADR-0008-delimiter-bounded-governance-record-paths.md`; `docs/adr/INDEX.md`.

### Stable Implementation Touchpoints [Conditionally Required — source or configuration implementation]

| Path | Stable symbol or contract anchor | Key code excerpt, when needed | Purpose | Source revision |
| --- | --- | --- | --- | --- |
| `tools/governance-validator/test/index-path-validation.test.mjs` | `validRepository` and selected-ADR relationship regression | N/A — stable test anchors are sufficient | Runs the production validator against malformed and valid repository fixtures. | `67807a4d26a77a4a3bff24317499289b0bba5882` |
| `tools/governance-validator/lib/relationship-validation.mjs` | `createRelationshipValidator` > `validateIndex` and reciprocal relationship checks | N/A — stable symbols are sufficient | Owns index fallback and linked ADR/ADD record-path extraction. | `67807a4d26a77a4a3bff24317499289b0bba5882` |
| `tools/governance-validator/validate.mjs` | `ADR_PATH_PATTERN` and `ADD_PATH_PATTERN` | N/A — stable symbols are sufficient | Supplies the current CLI relationship path recognizers that the correction removes. | `67807a4d26a77a4a3bff24317499289b0bba5882` |

**Migration and rollback strategy [Conditionally Required — this replaces or changes existing behavior]**: Replace only extraction of delimiter-bounded record tokens; stop if a valid project or service-relative fixture fails. Rollback is a Git revert of the implementation commit, restoring the prior expression-based extraction; no data or runtime migration is involved.

### Engineering Exceptions [Conditionally Required — an engineering rule is exceeded or waived]

N/A — the planned localized helper and regression do not exceed or waive a software-engineering rule.

## Contract-To-Check Traceability [Conditionally Required — source or configuration implementation]

| Clause ID | Authoritative contract path and heading | Exact normative requirement | Acceptance check or deterministic test IDs | Explicit coverage method |
| --- | --- | --- | --- | --- |
| TC-1 | `docs/adr/ADR-0008-delimiter-bounded-governance-record-paths.md` — Decision | Only a complete delimiter-bounded ADR or ADD path token may be resolved as a governance record reference. | AC-1 | A real validator run rejects a Selected candidate whose token ends in `.md.` instead of accepting the valid `.md` prefix. |
| TC-2 | `tools/governance-validator/README.md` — Governance Validator | The validator enforces deterministic ADD, ADR, and OCR structure and lifecycle contracts. | AC-2, AC-3 | The focused regressions, complete package suite, and repository governance command complete with their exact expected results. |

## Risk Coverage Matrix [Conditionally Required — source or configuration implementation]

| Risk dimension | Applicability and scenario, or specific N/A reason | Owning boundary | Deterministic verification method | Exact expected result | Acceptance check IDs | Status | Actual evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| concurrency and ordering | N/A — the validator performs synchronous parsing with no shared mutable concurrent state. | Governance-validator CLI | Structured source review and AC-2. | No concurrent state or ordering contract is introduced. | AC-2 | N/A — no concurrent behavior | Not run — implementation not started. |
| timeout and deadline | Applicable — the three current expressions are reported as potentially super-linear on malformed path text. | Governance record-path extraction | Existing adversarial timeout regression plus AC-2. | The existing adversarial fixture has no timeout error and the focused suite exits zero. | AC-2 | Not Started | Not run — implementation not started. |
| cancellation and interruption | N/A — the CLI exposes no cancellation protocol and the correction does not add one. | Governance-validator CLI | Structured source review and AC-2. | No cancellation interface or lifecycle is introduced. | AC-2 | N/A — no cancellation protocol | Not run — implementation not started. |
| resource bounds and backpressure | Applicable — delimiter tokenization must process malformed input without regex backtracking. | Governance record-path extraction | Existing adversarial timeout regression and AC-2. | The adversarial fixture completes within the existing 1,000 ms child-process timeout. | AC-2 | Not Started | Not run — implementation not started. |
| framework or trust-boundary rejection | Applicable — candidate links and Markdown index cells are untrusted repository inputs. | `createRelationshipValidator` relationship extraction | Trailing-punctuation real-validator regression. | The malformed reference fails validation and a valid fixture remains accepted. | AC-1 | Not Started | Not run — implementation not started. |

## Acceptance Checks [Required]

| Check ID | Subtask | Binary acceptance point | Preconditions or input | Verification method | Exact expected result | Expected evidence | Status | Actual result and evidence |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| AC-1 | T-1 | The actual validator rejects a Selected ADR reference whose complete path token ends in `.md.`. | A valid temporary repository fixture with CAND-1 changed to Selected, a reciprocal ADR source, and linked path `docs/adr/ADR-0001-example.md.`. | `node --test --test-name-pattern "trailing punctuation" test/index-path-validation.test.mjs` in `tools/governance-validator`. | Exit status 0; the selected test passes after asserting the spawned validator exits 1 and reports the linked ADR path as missing. | Focused Node test report. | Not Started | Pending |
| AC-2 | T-2 | The package regressions and complete test suite pass after replacing the three expression sites and governed-file markers. | Accepted implementation in the isolated task branch. | `npm test` in `tools/governance-validator`. | Exit status 0 with no failing test; the existing adversarial index-path regression has no timeout error. | Full package test report. | Not Started | Pending |
| AC-3 | T-3 | Repository governance validation accepts the new ADR/index state and unchanged record contracts. | All task changes present in the isolated task branch. | `npm run validate` in `tools/governance-validator`. | Exit status 0 and no validation errors. | Governance-validation command output. | Not Started | Pending |

## Completion Checklist [Required]

| ID | Item | Completion Criterion | Expected Evidence | Status | Actual Evidence |
| --- | --- | --- | --- | --- | --- |
| A-1 | ADR approved | An eligible non-author approver, approval time, and exact `Approval Evidence: Approve` are recorded. | ADR metadata | Complete | @linhai approved at 2026-08-31T20:50:11+08:00 with `Approval Evidence: Approve`. |
| A-2 | Complete task delivered | T-1 through T-3 have implementation evidence and AC-1 through AC-3 are Pass. | Implementation Plan and Acceptance Checks | Not Started | Pending |
| A-3 | Reciprocal ADD link synchronized, when applicable | N/A — the task is not derived from product demand and has no ADD candidate. | Metadata Architecture Source | N/A — no ADD applies | N/A — no product-demand ADD applies. |
| A-4 | Requirement levels satisfied | Every required section is complete, and every conditional trigger is completed or has a specific N/A reason. | Structured document review | Not Started | Pending |
| A-5 | Acceptance checks are decidable | Every check has one subtask, input, deterministic method, exact expected result, and evidence. | Acceptance Checks table | Not Started | Pending |
| A-6 | Engineering exceptions governed, when applicable | N/A — no engineering exception is planned. | Engineering Exceptions section | N/A — no exception applies | N/A — no engineering rule is exceeded or waived. |
| A-7 | Contract and baseline risks covered, when applicable | TC-1 and TC-2 map to checks, and every applicable risk reaches Pass before completion. | Traceability, matrix, and command reports | Not Started | Pending |
| A-8 | Governance validation passed | The independent validator reports no document or repository validation error. | `npm run validate` output | Not Started | Pending |

## Supporting Notes [Optional]

The new local SonarQube issue list shows exactly three Medium Reliability findings, one in `relationship-validation.mjs` and two in `validate.mjs`. Their source locations match the three findings previously classified as Security. The issue category is diagnostic context, not a source contract.

## Archival [Conditionally Required — Decision Status is `Rejected`, or Decision Status is `Deprecated` or `Superseded` and Implementation Status is final]

The record is Accepted and not archival-eligible. If a later rejection, deprecation, or supersession triggers archival, move it under `docs/adr/archive/`, update all governed-file markers and references in the same change, and update its single index row.

## Change Log [Required]

| Date | Change | Author |
| --- | --- | --- |
| 2026-08-31 | Drafted the Full ADR for the residual three SonarQube Reliability findings in governance-record path extraction. | @codex |
| 2026-08-31 | Accepted by @linhai with approval evidence `Approve` at 2026-08-31T20:50:11+08:00. | @codex |
