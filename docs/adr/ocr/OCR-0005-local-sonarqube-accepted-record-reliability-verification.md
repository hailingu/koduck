# OCR-0005: Local SonarQube Accepted-Record Reliability Verification

## Metadata [Required]

- **Decision Status**: Accepted
- **Implementation Status**: Not Started
- **Date**: 2026-08-31
- **Author**: @codex
- **Decision Owner**: @linhai
- **Required Approver**: @linhai
- **Record Scope**: Project
- **Approver [Conditionally Required — Decision Status is or has been `Accepted`]**: @linhai
- **Approval Time [Conditionally Required — Decision Status is or has been `Accepted`]**: 2026-08-31T21:39:04+08:00
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
- **Input Source or Version**: `81a7c51` — `fix(governance): simplify accepted-record validation`
- **Expected Output or Target State**: One new local analysis of source input `81a7c51` completes successfully, and the overall Open/Confirmed Reliability view has zero active findings at the four former `accepted-records.mjs` locations.
- **Docker Image Coordinates or Input Identity [Conditionally Required — container operation]**: N/A — this is not a container operation.
- **Kubernetes Target [Conditionally Required — Kubernetes operation]**: N/A — this is not a Kubernetes operation.
- **Actual immutable artifact [Conditionally Required — operation builds or consumes an artifact]**: N/A — the scanner submits an analysis report but does not build or consume a reusable artifact.
- **Dependencies**: Accepted `docs/adr/ADR-0009-accepted-record-validator-reliability.md`; locally installed `sonar-scanner`; an already-provisioned project token supplied only through the scanner process environment; reachable local SonarQube service.
- **Related [Optional]**: Local SonarQube project `koduck`; four Reliability findings in `tools/governance-validator/lib/accepted-records.mjs` observed 2026-08-31.
- **Architecture Source [Conditionally Required — a governing ADR or ADD task applies]**: `docs/adr/ADR-0009-accepted-record-validator-reliability.md`, subtask T-2
- **Supersedes [Conditionally Required — this OCR replaces another]**: N/A — no OCR is replaced
- **Superseded By [Conditionally Required — this OCR is replaced]**: None

## Requirement Level Legend [Required]

- **`[Required]`**: The section or field always applies and MUST remain present with complete, verifiable content. Use `None — <reason>` only when the template explicitly permits an empty result; never leave it blank.
- **`[Conditionally Required — <trigger>]`**: The section or field MUST be completed when its stated trigger applies. When the trigger does not apply, retain `N/A — <reason>` unless the template explicitly instructs removal or retention as inactive future-lifecycle guidance. A missing trigger assessment is incomplete content.
- **`[Optional]`**: The section may be removed without affecting acceptance, execution, completion, or verification. If retained, it MUST be accurate and complete; optional content MUST NOT substitute for required evidence.

Unlabeled fields inside a `[Required]` section are required.

Planned content is required before `Accepted`; actual-result fields remain `Pending` until their corresponding operation stage runs and are required before `Complete` or `Verified`.

## Task Definition [Required]

**Complete task outcome**: Submit one local SonarQube analysis of source input `81a7c51` and verify that the overall Open/Confirmed Reliability view has no active finding at the four former `accepted-records.mjs` locations; otherwise stop and restore the pre-operation analysis baseline by deleting only the newly created analysis.

Allowed subtask statuses: `Not Started`, `In Progress`, `Blocked`, `Complete`, or `N/A — <specific reason>`.

| ID | Objective or deliverable | Included scope or target | Completion criterion | Expected evidence | Status | Actual evidence |
| --- | --- | --- | --- | --- | --- | --- |
| T-1 | Submit one source analysis to the existing local project. | Local SonarQube project `koduck`, source input `81a7c51`, main branch analysis. | Scanner exits 0 and the submitted compute-engine task completes successfully. | Scanner result, task identifier, and source revision. | Not Started | Pending — execution has not been authorized. |
| T-2 | Verify the accepted-record Reliability finding state. | Existing local project overall Reliability issue view, Open/Confirmed statuses, `accepted-records.mjs` finding rows. | Zero active rows remain for the four former `accepted-records.mjs` locations; no recovery action is required. | Issue-view result and target analysis timestamp or task identifier. | Not Started | Pending — execution has not been authorized. |

## Eligibility [Required]

- [x] Uses an accepted architecture, pipeline, artifact contract, security boundary, and data boundary: Accepted `docs/adr/ADR-0009-accepted-record-validator-reliability.md` defines the source correction and the established local project analysis pipeline; this operation neither produces a reusable artifact nor alters the token or data boundary.
- [x] Is reversible: before Execute, capture the current analysis identifier; if Execute or Verify fails after a new analysis is created, delete only that new analysis through the local SonarQube analysis-management operation and restore the captured baseline as current.
- [x] Does not modify a Dockerfile, Makefile, CI, pipeline, artifact format, signing, credentials, deployment topology, API/schema/protocol, authentication, security policy, data lifecycle, dependency, provider, or irreversible behavior.
- [x] Has a defined preflight, success check, stop condition, and recovery path.
- [x] Contains no secret, credential, private endpoint, or sensitive user data. The token is not recorded, displayed, or passed in this OCR.
- [x] N/A — no automatic-review mechanism is configured for this local, non-pushed operational input. Review is not treated as approval or execution evidence.

## Core Runbook And Evidence [Required]

### Preflight [Required]

**Planned action and criterion**: Confirm this OCR is Accepted before execution; confirm the local service exposes project `koduck` with the prior analysis identifier captured; confirm `sonar-scanner` is available; and confirm the scanner token is available only through its process environment without printing, storing, or transmitting it to any other destination.

**Actual result and stable evidence**: Not started — approved execution has not yet begun.

### Execute [Required]

**Planned action**: Run the installed scanner from the isolated task worktree against source input `81a7c51`, using the existing local project key and branch configuration. Supply the pre-provisioned token only through the scanner process environment. Do not log the token, scanner environment, or any copied credential.

**Actual result and stable evidence**: Not started — approved execution has not yet begun.

### Verify [Required]

**Success criterion**: The scanner reports successful analysis submission and compute-engine completion; the resulting overall Reliability view has exactly zero active rows at the four former `accepted-records.mjs` locations. Record only non-sensitive scanner/task and issue-view evidence.

**Actual result and stable evidence**: Not started — approved execution has not yet begun.

### Stop and Recovery [Required]

**Stop condition**: Stop if the token is unavailable, the target project or baseline analysis cannot be confirmed, the scanner exits nonzero, the compute-engine task fails, or any of the four target rows remains active.

**Recovery action**: If a new analysis exists after a stop condition, delete only that analysis using the captured local baseline and record its identifier; otherwise make no project-state change. Do not alter source, token, profile, quality gate, or issue workflow state.

**Recovery verification**: Confirm that the captured pre-operation analysis is again the current local project analysis and that no new analysis remains.

**Actual result and stable evidence**: Not triggered — approved execution has not yet begun.

## Conditional Extensions [Conditionally Required — production, multi-environment, phased, user/downstream/SLO impact, or stated change-window operation]

N/A — this is a single local analysis with no production, multi-environment, phased, user/downstream, SLO, or change-window impact.

## Closure [Required]

Allowed review statuses for Authorization review, Subtask and evidence review, and Requirement-level review are `Pass`, `Fail`, or `N/A — <specific reason>`.

- **Final result**: Not started — the accepted local operation has not yet run.
- **Authorization review**: Pass — Accepted approval metadata is complete and precedes execution.
- **Subtask and evidence review**: N/A — operation has not run.
- **Requirement-level review**: N/A — terminal review occurs after the operation.
- **Governance validation**: Pass — `npm run validate --prefix tools/governance-validator` exited zero for the proposed OCR revision; run again for the accepted revision before Execute.

## Supporting Notes [Optional]

The expected view may still contain other Reliability findings outside `accepted-records.mjs`; those are not evidence of failure for this scoped OCR.

## Archival [Conditionally Required — Decision Status is retired or Implementation Status is final]

The accepted operation is not terminal. On `Complete` or `Verified`, move this record under `docs/adr/ocr/archive/` in the same change as the terminal status and index-path update.

## Change Log [Required]

| Date | Change | Author |
| --- | --- | --- |
| 2026-08-31 | Drafted the OCR for one local accepted-record Reliability-remediation verification analysis. | @codex |
| 2026-08-31 | @linhai approved the unchanged planned local operation. | @linhai |
