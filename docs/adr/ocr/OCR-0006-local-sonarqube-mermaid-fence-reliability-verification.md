# OCR-0006: Local SonarQube Mermaid-Fence Reliability Verification

## Metadata [Required]

- **Decision Status**: Accepted
- **Implementation Status**: In Progress
- **Date**: 2026-08-31
- **Author**: @codex
- **Decision Owner**: @linhai
- **Required Approver**: @linhai
- **Record Scope**: Project
- **Approver [Conditionally Required — Decision Status is or has been `Accepted`]**: @linhai
- **Approval Time [Conditionally Required — Decision Status is or has been `Accepted`]**: 2026-08-31T22:24:44+08:00
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
- **Operation Type**: Existing Runbook
- **Target Scope / Operation Owner**: Local SonarQube project `koduck` / @codex
- **Input Source or Version**: `4d9c274` — `fix(governance): bound Mermaid fence recognition`
- **Expected Output or Target State**: One new local analysis of input `4d9c274` completes successfully, and the overall Open/Confirmed Reliability view has zero active findings at the former `tools/governance-validator/lib/mermaid-validation.mjs` opening-fence location.
- **Docker Image Coordinates or Input Identity [Conditionally Required — container operation]**: N/A — this is not a container operation.
- **Kubernetes Target [Conditionally Required — Kubernetes operation]**: N/A — this is not a Kubernetes operation.
- **Actual immutable artifact [Conditionally Required — operation builds or consumes an artifact]**: N/A — the scanner submits an analysis report but does not build or consume a reusable artifact.
- **Dependencies**: Accepted `docs/adr/ADR-0010-mermaid-fence-recognition-reliability.md`; locally installed `sonar-scanner`; an already-provisioned project token supplied only through the scanner process environment; reachable local SonarQube service.
- **Related [Optional]**: Local SonarQube project `koduck`; one Reliability finding in `tools/governance-validator/lib/mermaid-validation.mjs` observed 2026-08-31.
- **Architecture Source [Conditionally Required — a governing ADR or ADD task applies]**: `docs/adr/ADR-0010-mermaid-fence-recognition-reliability.md`, subtask T-2
- **Supersedes [Conditionally Required — this OCR replaces another]**: N/A — no OCR is replaced
- **Superseded By [Conditionally Required — this OCR is replaced]**: None

## Requirement Level Legend [Required]

- **`[Required]`**: The section or field always applies and MUST remain present with complete, verifiable content. Use `None — <reason>` only when the template explicitly permits an empty result; never leave it blank.
- **`[Conditionally Required — <trigger>]`**: The section or field MUST be completed when its stated trigger applies. When the trigger does not apply, retain `N/A — <reason>` unless the template explicitly instructs removal or retention as inactive future-lifecycle guidance. A missing trigger assessment is incomplete content.
- **`[Optional]`**: The section may be removed without affecting acceptance, execution, completion, or verification. If retained, it MUST be accurate and complete; optional content MUST NOT substitute for required evidence.

Unlabeled fields inside a `[Required]` section are required.

Planned content is required before `Accepted`; actual-result fields remain `Pending` until their corresponding operation stage runs and are required before `Complete` or `Verified`.

## Task Definition [Required]

**Complete task outcome**: Submit one local SonarQube analysis of source input `4d9c274` and verify that the overall Open/Confirmed Reliability view has no active finding at the former Mermaid opening-fence location; otherwise stop and restore the pre-operation analysis baseline by deleting only the newly created analysis.

Allowed subtask statuses: `Not Started`, `In Progress`, `Blocked`, `Complete`, or `N/A — <specific reason>`.

| ID | Objective or deliverable | Included scope or target | Completion criterion | Expected evidence | Status | Actual evidence |
| --- | --- | --- | --- | --- | --- | --- |
| T-1 | Confirm authority, target baseline, scanner availability, and secure token availability. | Local SonarQube project `koduck` and source input `4d9c274`. | Accepted OCR, known current analysis baseline, installed scanner, and non-empty scanner-process token availability without exposing its value. | Approval metadata and non-sensitive preflight record. | In Progress | Accepted approval metadata is recorded; preflight begins before scanner submission. |
| T-2 | Submit exactly one analysis of the approved source input. | Isolated task worktree and existing local SonarQube project `koduck`. | Scanner exits 0 and the submitted compute-engine task completes successfully. | Scanner exit result, task identifier, and source version. | Not Started | Awaiting successful T-1 preflight. |
| T-3 | Verify the scoped Reliability result or recover the local analysis baseline. | Local Open/Confirmed overall Reliability issue view and its Mermaid-fence target location. | Zero active target rows; otherwise only the newly created analysis is deleted and the captured baseline is current again. | Issue-view result, analysis identifier, and recovery record when triggered. | Not Started | Awaiting successful T-2 analysis submission. |

## Eligibility [Required]

- [x] Uses an accepted architecture, pipeline, artifact contract, security boundary, and data boundary: `docs/adr/ADR-0010-mermaid-fence-recognition-reliability.md` authorizes the completed source correction and the existing local analysis pipeline; this operation neither produces a reusable artifact nor changes a token or data boundary.
- [x] Is reversible: before Execute, capture the current analysis identifier; if Execute or Verify fails after a new analysis is created, delete only that new analysis through the local SonarQube analysis-management operation and restore the captured baseline as current.
- [x] Does not modify a Dockerfile, Makefile, CI, pipeline, artifact format, signing, credentials, deployment topology, API/schema/protocol, authentication, security policy, data lifecycle, dependency, provider, or irreversible behavior.
- [x] Has a defined preflight, success check, stop condition, and recovery path.
- [x] Contains no secret, credential, private endpoint, or sensitive user data. The token is not recorded, displayed, or passed in this OCR.
- [x] N/A — no automatic-review mechanism is configured for this local, non-pushed operational input. Review is not treated as approval or execution evidence.

## Core Runbook And Evidence [Required]

### Preflight [Required]

**Planned action and criterion**: Confirm this OCR is Accepted before execution; confirm the local project exposes `koduck` and capture its current analysis identifier; confirm `sonar-scanner` is available; and confirm the scanner token is available only through its process environment without printing, storing, or transmitting it to any other destination.

**Actual result and stable evidence**: In progress — approval is recorded; project baseline, scanner availability, and non-empty secure token availability will be confirmed without exposing the token value.

### Execute [Required]

**Planned action**: Run the installed scanner from the isolated task worktree against source input `4d9c274`, using the existing local project key and branch configuration. Supply the pre-provisioned token only through the scanner process environment. Do not log the token, scanner environment, or any copied credential.

**Actual result and stable evidence**: Not started — T-1 preflight has not yet cleared submission.

### Verify [Required]

**Success criterion**: The scanner reports successful analysis submission and compute-engine completion; the resulting overall Reliability view has exactly zero active rows at the former `mermaid-validation.mjs` opening-fence location. Record only non-sensitive scanner/task and issue-view evidence.

**Actual result and stable evidence**: Awaiting successful scanner submission and compute-engine completion.

### Stop and Recovery [Required]

**Stop condition**: Stop if the token is unavailable, the target project or baseline analysis cannot be confirmed, the scanner exits nonzero, the compute-engine task fails, or the target row remains active.

**Recovery action**: If a new analysis exists after a stop condition, delete only that analysis using the captured local baseline and record its identifier; otherwise make no project-state change. Do not alter source, token, profile, quality gate, or issue workflow state.

**Recovery verification**: Confirm that the captured pre-operation analysis is again the current local project analysis and that no new analysis remains.

**Actual result and stable evidence**: Not triggered — no stop condition has occurred before scanner submission.

## Conditional Extensions [Conditionally Required — production, multi-environment, phased, user/downstream/SLO impact, or stated change-window operation]

N/A — this is a single local analysis with no production, multi-environment, phased, user/downstream, SLO, or change-window impact.

## Closure [Required]

Allowed review statuses for Authorization review, Subtask and evidence review, and Requirement-level review are `Pass`, `Fail`, or `N/A — <specific reason>`.

- **Final result**: In progress — accepted operation begins with T-1 preflight.
- **Authorization review**: Pass — Accepted approval metadata is complete and precedes the first Execute action.
- **Subtask and evidence review**: In progress — terminal review follows scanner completion and issue-view verification.
- **Requirement-level review**: In progress — terminal review follows the completed operation.
- **Governance validation**: In progress — rerun `npm run validate --prefix tools/governance-validator` for this accepted OCR revision before Execute.

## Supporting Notes [Optional]

This OCR verifies only the one Mermaid-fence target. Fourteen overall Reliability findings in other validator modules remain intentionally outside this operation.

## Archival [Conditionally Required — Decision Status is retired or Implementation Status is final]

The accepted operation is not terminal. If it reaches a final implementation status, archive it under `docs/adr/ocr/archive/` in the same change that establishes the final status and index-path update.

## Change Log [Required]

| Date | Change | Author |
| --- | --- | --- |
| 2026-08-31 | Drafted the OCR for one local Mermaid-fence Reliability-remediation verification analysis. | @codex |
| 2026-08-31 | Accepted by @linhai with approval evidence `Approve` at 2026-08-31T22:24:44+08:00. | @codex |
