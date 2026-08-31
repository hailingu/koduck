# ADR-0007: Linear-Time Governance Path Recognition

## Metadata [Required]

- **Decision Status**: Accepted
- **Implementation Status**: Complete
- **Date**: 2026-08-31
- **Author**: @codex
- **Decision Owner**: @linhai
- **Required Approver**: @linhai
- **Record Scope**: Project
- **Approver [Conditionally Required — Decision Status is or has been `Accepted`]**: @linhai
- **Approval Time [Conditionally Required — Decision Status is or has been `Accepted`]**: 2026-08-31T17:22:50+08:00
- **Approval Evidence [Conditionally Required — Decision Status is or has been `Accepted`]**: Approve
- **Rejector [Conditionally Required — Decision Status is `Rejected`]**: N/A — Decision Status is Proposed
- **Rejection Time [Conditionally Required — Decision Status is `Rejected`]**: N/A — Decision Status is Proposed
- **Rejection Evidence [Conditionally Required — Decision Status is `Rejected`]**: N/A — Decision Status is Proposed
- **Retired By [Conditionally Required — Decision Status is `Deprecated` or `Superseded`]**: N/A — record is not retired
- **Retirement Time [Conditionally Required — Decision Status is `Deprecated` or `Superseded`]**: N/A — record is not retired
- **Retirement Evidence [Conditionally Required — Decision Status is `Deprecated` or `Superseded`]**: N/A — record is not retired
- **Retirement Reason [Conditionally Required — Decision Status is `Deprecated` or `Superseded`]**: N/A — record is not retired
- **Blocked From [Conditionally Required — Implementation Status is `Blocked`]**: N/A — implementation is not blocked
- **Blocker And Evidence [Conditionally Required — Implementation Status is `Blocked`]**: N/A — implementation is not blocked
- **Blocker Owner [Conditionally Required — Implementation Status is `Blocked`]**: N/A — implementation is not blocked
- **Blocker Exit Or Recheck Criterion [Conditionally Required — Implementation Status is `Blocked`]**: N/A — implementation is not blocked
- **Related [Optional]**: Local SonarQube issue list for project `koduck`, security filter, observed 2026-08-31.
- **Architecture Source [Conditionally Required — product demand]**: N/A — corrective security work requested against the local SonarQube project, not derived from product demand
- **Supersedes [Conditionally Required — this ADR replaces another]**: N/A — no record is replaced
- **Superseded By [Conditionally Required — this ADR is replaced]**: None

## Requirement Level Legend [Required]

- **`[Required]`**: The section or field always applies and MUST remain present with complete, verifiable content. Use `None — <reason>` only when the template explicitly permits an empty result; never leave it blank.
- **`[Conditionally Required — <trigger>]`**: The section or field MUST be completed when its stated trigger applies. When the trigger does not apply, retain `N/A — <reason>` unless the template explicitly instructs removal or retention as inactive future-lifecycle guidance. A missing trigger assessment is incomplete content.
- **`[Optional]`**: The section may be removed without affecting acceptance, execution, completion, or verification. If retained, it MUST be accurate and complete; optional content MUST NOT substitute for required evidence.

Unlabeled fields inside a `[Required]` section are required.

## Context And Problem Statement [Required]

The first local SonarQube scan of project `koduck` reported three High-severity Security findings: one expression in the governance-record index validator and two expressions in the governance-validator CLI. Each expression combines an unbounded repeated group with an overlapping unbounded token, so an adversarial Markdown cell or command-line-like path can cause excessive regular-expression backtracking.

The validator processes repository Markdown and command-line arguments. It must preserve valid ADR and ADD path recognition while rejecting malformed input without a timeout. This ADR owns one project-wide security correction across the single governance-validator path-recognition boundary.

## Scope [Required]

In scope:

- Add regression coverage that runs the actual validator against an adversarial index-path input and proves that the child process finishes before its timeout.
- Replace the three reported path-recognition expressions with path-segment-constrained equivalents that avoid overlapping repetition.
- Preserve valid governance-validator behavior through its focused and routed test suites.

Out of scope:

- Changing the supported ADR, ADD, OCR, or CLI path syntax beyond the malformed input needed for the regression.
- Changing SonarQube configuration, profiles, issue statuses, quality gates, or credentials.
- Refactoring unrelated validator behavior or adding dependencies.

## Tensions, Constraints, And Open Questions [Required]

### Identified Tensions [Conditionally Required — competing goals or trade-offs exist]

| ID | Tension | Impact | Decision |
| --- | --- | --- | --- |
| TN-1 | Security must prevent adversarial backtracking while the validator must continue recognizing valid nested repository paths. | An overly broad replacement could retain ReDoS risk; an overly narrow replacement could reject valid records. | Constrain repeated groups to one slash-free segment and verify both the adversarial and existing valid fixtures through the real CLI. |

### Constraints [Required]

- The change is source work with a security concern and requires this Full ADR to be Accepted before the test or production implementation changes.
- No dependency, public contract, index schema, or command-line syntax change is permitted.
- The implementation must follow Red-Green-Refactor and use the existing locked Node.js test tooling.
- Each governed JavaScript source file must cite this record at its first legal comment position.

### Open Questions [Conditionally Required — material questions exist or were resolved during drafting]

None — no material questions remain. SonarQube identified the three expressions and the affected paths share one validator boundary.

## Decision Drivers [Required]

1. **Prevent resource exhaustion**: malformed external text must not keep the validation CLI in exponential regex backtracking.
2. **Preserve deterministic validation**: valid existing ADR and ADD fixture paths must continue to validate without schema changes.
3. **Keep the correction narrow**: use the existing validator and test harness without adding a package or parallel parsing abstraction.

## Options Considered [Required]

### Option: Replace broad repeated tokens with slash-free path segments

Use repeated groups whose individual segments exclude `/`, leaving each path separator consumed by exactly one part of the expression.

Pros:

- Removes the overlapping repetition that produces the reported ReDoS behavior.
- Retains the documented nested relative-path form.
- Is a localized change with no new dependency.

Cons:

- Requires focused regression coverage to prove the malformed-input and valid-path behaviors together.

### Option: Parse every candidate path without regular expressions

Split each string and reconstruct the allowed path grammar in procedural code.

Pros:

- Avoids regular expressions for this grammar.

Cons:

- Broadens the change across established parser behavior without a demonstrated need.
- Adds handling paths beyond the three reported findings.

### Option: Keep the expressions and bound only the caller input

Reject candidate input at a coarse length limit before applying the existing expressions.

Pros:

- Limits some large inputs.

Cons:

- Leaves the unsafe matching structure in place and changes input policy without demonstrating a safe threshold.

## Decision [Required]

**Selected option**: Replace broad repeated tokens with slash-free path segments.

**Rationale**: A path separator belongs to a single repeated path segment. Encoding that grammar directly eliminates the reported nested unbounded overlap, leaves valid nested paths representable, and is the smallest change that can be tested against the actual validator process.

### Consequences [Required]

Positive:

- The reported expressions no longer expose the validator to the demonstrated exponential-backtracking input shape.
- A regression test makes the timeout behavior observable in the existing Node.js suite.

Negative:

- Path expressions become slightly more explicit and require synchronized tests to guard their intended grammar.

Mitigations:

- Keep the expressions local, add one real-process regression test, and run the full validator tests plus repository governance validation.

## Implementation Plan [Required]

**Complete task outcome**: The three SonarQube Security findings for vulnerable governance-validator path expressions are removed by a test-first change that rejects the adversarial index-path fixture without a child-process timeout while the existing governance-validator suites pass.

**Primary implementation boundary**: `tools/governance-validator` deterministic path recognition for governance records and CLI relationship validation.

Allowed subtask statuses: `Not Started`, `In Progress`, `Blocked`, `Complete`, or `N/A — <specific reason>`.

| ID | Objective or deliverable | Included scope | Status | Actual implementation evidence |
| --- | --- | --- | --- | --- |
| T-1 | Add the smallest real-process regression test for an adversarial repeated path segment that previously reaches the child-process timeout. | `tools/governance-validator/test/index-path-validation.test.mjs` and its existing temporary repository fixture helpers. | Complete | Red: focused test produced `ETIMEDOUT` at the 1,000 ms child timeout. Green: the same test passed in 313 ms after T-2. |
| T-2 | Replace the reported expressions with slash-free path-segment patterns and update governed-file markers. | `tools/governance-validator/lib/relationship-validation.mjs` and `tools/governance-validator/validate.mjs`. | Complete | Replaced the three expressions with slash-free repeated path segments and changed each governed-file marker to this ADR; immutable source commit evidence is recorded before terminal completion. |
| T-3 | Verify the focused regression, full validator suite, and repository governance validation. | Existing `npm` scripts in `tools/governance-validator`. | Complete | Focused regression passed; `npm test` passed 146 of 146 tests; `npm run validate` reported `Governance validation passed.` |

**Affected paths**: `tools/governance-validator/test/validate.test.mjs`; `tools/governance-validator/lib/relationship-validation.mjs`; `tools/governance-validator/validate.mjs`; `docs/adr/ADR-0007-linear-time-governance-path-recognition.md`; `docs/adr/INDEX.md`.

### Stable Implementation Touchpoints [Conditionally Required — source or configuration implementation]

| Path | Stable symbol or contract anchor | Key code excerpt, when needed | Purpose | Source revision |
| --- | --- | --- | --- | --- |
| `tools/governance-validator/test/index-path-validation.test.mjs` | `rejects an adversarial index-path without timing out` | N/A — stable symbol is sufficient | Executes the production validator against a controlled adversarial temporary repository fixture. | `109c478580d8ec41bdf65b43b68cb8a0e90c14a8` |
| `tools/governance-validator/lib/relationship-validation.mjs` | `createRelationshipValidator` > `validateIndex` | N/A — stable symbol is sufficient | Finds candidate governance record paths while validating index rows. | `109c478580d8ec41bdf65b43b68cb8a0e90c14a8` |
| `tools/governance-validator/validate.mjs` | `ADR_PATH_PATTERN` and `ADD_PATH_PATTERN` | N/A — stable symbols are sufficient | Recognizes linked ADR and ADD record paths for reciprocal relationship validation. | `109c478580d8ec41bdf65b43b68cb8a0e90c14a8` |

**Migration and rollback strategy [Conditionally Required — this replaces or changes existing behavior]**: Replace only the unsafe pattern internals; stop if existing valid-path tests fail. Rollback is a Git revert of the task commit, returning to the current expressions; no data or runtime migration is involved.

### Engineering Exceptions [Conditionally Required — an engineering rule is exceeded or waived]

N/A — the planned localized changes do not exceed or waive a software-engineering rule.

## Contract-To-Check Traceability [Conditionally Required — source or configuration implementation]

| Clause ID | Authoritative contract path and heading | Exact normative requirement | Acceptance check or deterministic test IDs | Explicit coverage method |
| --- | --- | --- | --- | --- |
| TC-1 | `docs/adr/ADR-0007-linear-time-governance-path-recognition.md` — Decision | An adversarial repeated path-segment string passed through index validation must cause validation failure rather than a child-process timeout. | AC-1 | The real CLI processes a temporary index fixture containing 24 repeated path segments and the test asserts exit status 1 with no timeout error. |
| TC-2 | `tools/governance-validator/README.md` — Governance Validator | The validator enforces deterministic ADD, ADR, and OCR structure and lifecycle contracts. | AC-2, AC-3 | The package test suite exercises valid fixture paths and the repository validation command completes with exit status 0. |

## Risk Coverage Matrix [Conditionally Required — source or configuration implementation]

| Risk dimension | Applicability and scenario, or specific N/A reason | Owning boundary | Deterministic verification method | Exact expected result | Acceptance check IDs | Status | Actual evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| concurrency and ordering | N/A — the validator is a synchronous single-process CLI with no shared mutable concurrent state. | Governance-validator CLI | Structured source review and AC-2. | No concurrent state or ordering contract is introduced. | AC-2 | N/A — no concurrent behavior | No concurrent state or ordering behavior was added. |
| timeout and deadline | Applicable — the malformed 24-segment candidate path previously exceeds the child-process timeout during expression matching. | Path recognition expressions | Focused real-process regression test. | The child process has no timeout error and exits with status 1 for the invalid fixture. | AC-1 | Pass | Focused test passed in 313 ms; no child timeout error occurred. |
| cancellation and interruption | N/A — the CLI exposes no cancellation protocol; the focused child process ends naturally or through the test harness timeout. | Governance-validator CLI | Structured source review and AC-2. | No cancellation interface or lifecycle is introduced. | AC-2 | N/A — no cancellation protocol | No cancellation interface or lifecycle behavior was added. |
| resource bounds and backpressure | Applicable — repeated path segments must not cause unbounded CPU consumption from regex backtracking. | Path recognition expressions | Focused real-process regression test. | The adversarial fixture completes within the existing 1,000 ms child timeout. | AC-1 | Pass | The adversarial fixture completed in 313 ms, within the 1,000 ms child timeout. |
| framework or trust-boundary rejection | Applicable — repository Markdown index cells and linked record references are untrusted parser inputs. | `validateIndex`, `ADR_PATH_PATTERN`, and `ADD_PATH_PATTERN` | Focused regression and full validator suite. | Malformed input is rejected and valid repository fixtures still pass. | AC-1, AC-2 | Pass | The malformed fixture exited with status 1; `npm test` passed 146 of 146 tests. |

## Acceptance Checks [Required]

| Check ID | Subtask | Binary acceptance point | Preconditions or input | Verification method | Exact expected result | Expected evidence | Status | Actual result and evidence |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| AC-1 | T-1 | The actual validator rejects the adversarial index-path fixture before the child-process timeout. | A temporary valid repository with one index row containing `segment/` repeated 24 times followed by `docs` and no valid record path. | `node --test --test-name-pattern "adversarial index-path" test/index-path-validation.test.mjs` in `tools/governance-validator`. | Exit status 0; the selected test passes; its spawned validator result has exit status 1 and no `ETIMEDOUT` error. | Focused Node test report. | Pass | Initial red result: `ETIMEDOUT`; after the change, the selected test passed in 313 ms. |
| AC-2 | T-2 | The complete governance-validator test suite passes with the path expressions and governed-file markers updated. | Accepted implementation in the isolated task branch. | `npm test` in `tools/governance-validator`. | Exit status 0 with no failing test. | Full package test report. | Pass | `npm test` passed 146 of 146 tests. |
| AC-3 | T-3 | Repository governance validation accepts the new ADR/index state and unchanged record contracts. | All task changes present in the isolated task branch. | `npm run validate` in `tools/governance-validator`. | Exit status 0 and no validation errors. | Governance-validation command output. | Pass | `Governance validation passed.` |

## Completion Checklist [Required]

| ID | Item | Completion Criterion | Expected Evidence | Status | Actual Evidence |
| --- | --- | --- | --- | --- | --- |
| A-1 | ADR approved | An eligible non-author approver, approval time, and exact `Approval Evidence: Approve` are recorded. | ADR metadata | Complete | @linhai approved at 2026-08-31T17:22:50+08:00 with `Approval Evidence: Approve`. |
| A-2 | Complete task delivered | T-1 through T-3 have implementation evidence and AC-1 through AC-3 are Pass. | Implementation Plan and Acceptance Checks | Complete | Source implementation is recorded by commit `109c478580d8ec41bdf65b43b68cb8a0e90c14a8`; every declared subtask and check is complete or Pass. |
| A-3 | Reciprocal ADD link synchronized, when applicable | N/A — the task is not derived from product demand and has no ADD candidate. | Metadata Architecture Source | N/A — no ADD applies | N/A — no product-demand ADD applies. |
| A-4 | Requirement levels satisfied | Every required section is complete, and every conditional trigger is completed or has a specific N/A reason. | Structured document review | Complete | `npm run validate` passed after the accepted ADR update. |
| A-5 | Acceptance checks are decidable | Every check has one subtask, input, deterministic method, exact expected result, and evidence. | Acceptance Checks table | Complete | The accepted ADR's three checks reference T-1 through T-3 and passed as recorded. |
| A-6 | Engineering exceptions governed, when applicable | N/A — no engineering exception is planned. | Engineering Exceptions section | N/A — no exception applies | N/A — no engineering rule was exceeded or waived. |
| A-7 | Contract and baseline risks covered, when applicable | TC-1 and TC-2 map to checks, and every applicable risk reaches Pass before completion. | Traceability, matrix, and command reports | Complete | TC-1 and TC-2 map to AC-1 through AC-3; applicable matrix rows are Pass. |
| A-8 | Governance validation passed | The independent validator reports no document or repository validation error. | `npm run validate` output | Complete | `Governance validation passed.` |

## Supporting Notes [Optional]

SonarQube classified all three findings as High Security issues and identified them on 2026-08-31 in the local project scan. The exact identifiers are intentionally not treated as an implementation contract; the source locations and behavioral checks above are the durable evidence.

## Archival [Conditionally Required — Decision Status is `Rejected`, or Decision Status is `Deprecated` or `Superseded` and Implementation Status is final]

The record is Accepted and not archival-eligible. If a later rejection, deprecation, or supersession triggers archival, move this file under `docs/adr/archive/`, update all governed-file markers and references in the same change, and update its single index row.

## Change Log [Required]

| Date | Change | Author |
| --- | --- | --- |
| 2026-08-31 | Drafted the Full ADR for the three local SonarQube ReDoS findings in governance-validator path recognition. | @codex |
| 2026-08-31 | Accepted by @linhai with approval evidence `Approve` at 2026-08-31T17:22:50+08:00. | @codex |
| 2026-08-31 | Recorded the red `ETIMEDOUT` regression, the green implementation result, and pre-commit verification evidence. | @codex |
| 2026-08-31 | Completed the ADR with source evidence pinned to `109c478580d8ec41bdf65b43b68cb8a0e90c14a8`. | @codex |
