# OCR-0012: Local SonarQube Validator Supersession Classification Reliability Verification

## Metadata [Required]

- **Decision Status**: Accepted
- **Implementation Status**: Not Started
- **Date**: 2026-09-01
- **Author**: @codex
- **Decision Owner**: @linhai
- **Required Approver**: @linhai
- **Record Scope**: Project
- **Approver [Conditionally Required — Decision Status is or has been `Accepted`]**: @linhai
- **Approval Time [Conditionally Required — Decision Status is or has been `Accepted`]**: 2026-09-01T11:03:13+08:00
- **Approval Evidence [Conditionally Required — Decision Status is or has been `Accepted`]**: Approve
- **Rejector [Conditionally Required — Decision Status is `Rejected`]**: N/A — Decision Status is Accepted.
- **Rejection Time [Conditionally Required — Decision Status is `Rejected`]**: N/A — Decision Status is Accepted.
- **Rejection Evidence [Conditionally Required — Decision Status is `Rejected`]**: N/A — Decision Status is Accepted.
- **Retired By [Conditionally Required — Decision Status is `Deprecated` or `Superseded`]**: N/A — record is not retired.
- **Retirement Time [Conditionally Required — Decision Status is `Deprecated` or `Superseded`]**: N/A — record is not retired.
- **Retirement Evidence [Conditionally Required — Decision Status is `Deprecated` or `Superseded`]**: N/A — record is not retired.
- **Retirement Reason [Conditionally Required — Decision Status is `Deprecated` or `Superseded`]**: N/A — record is not retired.
- **Blocked From [Conditionally Required — Implementation Status is `Blocked`]**: N/A — implementation has not started.
- **Blocker And Evidence [Conditionally Required — Implementation Status is `Blocked`]**: N/A — implementation has not started.
- **Blocker Owner [Conditionally Required — Implementation Status is `Blocked`]**: N/A — implementation has not started.
- **Blocker Exit Or Recheck Criterion [Conditionally Required — Implementation Status is `Blocked`]**: N/A — implementation has not started.
- **Operation Type**: Existing Runbook
- **Target Scope / Operation Owner**: Local SonarQube project `koduck` / @codex
- **Input Source or Version**: `31328ab` — bounded supersession record-type classification in `validateSupersession`; it includes ADR-0013 source commit `f2cc310` and ADR-0014's prior source commits.
- **Expected Output or Target State**: Exactly one successful local analysis of source `31328ab`; the overall Open/Confirmed Reliability view has zero active rows in `tools/governance-validator/validate.mjs` for the nine ADR-0014 findings.
- **Docker Image Coordinates or Input Identity [Conditionally Required — container operation]**: N/A — this is not a container operation.
- **Kubernetes Target [Conditionally Required — Kubernetes operation]**: N/A — this is not a Kubernetes operation.
- **Actual immutable artifact [Conditionally Required — operation builds or consumes an artifact]**: N/A — the scanner submits a disposable analysis report and neither builds nor consumes a reusable artifact.
- **Dependencies**: Accepted `docs/adr/ADR-0014-validator-structural-parsing-reliability.md`; source commit `31328ab`; locally installed `sonar-scanner`; reachable local SonarQube project `koduck`; and the pre-provisioned `KODUCK_SONAR_TOKEN` supplied only to the scanner process environment.
- **Related [Optional]**: Archived `docs/adr/ocr/archive/OCR-0011-local-sonarqube-validator-structural-parsing-reliability-verification.md`; local SonarQube project's recovered `f2cc310` baseline with nine Reliability rows in `validate.mjs`.
- **Architecture Source [Conditionally Required — a governing ADR or ADD task applies]**: `docs/adr/ADR-0014-validator-structural-parsing-reliability.md`, subtask T-3
- **Supersedes [Conditionally Required — this OCR replaces another]**: N/A — this is a new verification of a successor source revision and does not replace OCR-0011's completed recovery record.
- **Superseded By [Conditionally Required — this OCR is replaced]**: None

## Requirement Level Legend [Required]

- **`[Required]`**: The section or field always applies and MUST remain present with complete, verifiable content. Use `None — <reason>` only when the template explicitly permits an empty result; never leave it blank.
- **`[Conditionally Required — <trigger>]`**: The section or field MUST be completed when its stated trigger applies. When the trigger does not apply, retain `N/A — <reason>` unless the template explicitly instructs removal or retention as inactive future-lifecycle guidance. A missing trigger assessment is incomplete content.
- **`[Optional]`**: The section may be removed without affecting acceptance, execution, completion, or verification. If retained, it MUST be accurate and complete; optional content MUST NOT substitute for required evidence.

Unlabeled fields inside a `[Required]` section are required.

Planned content is required before `Accepted`; actual-result fields remain `Pending` until their corresponding operation stage runs and are required before `Complete` or `Verified`.

## Task Definition [Required]

**Complete task outcome**: Submit exactly one local SonarQube analysis of immutable source `31328ab` and verify that no scoped `validate.mjs` Reliability row remains; otherwise stop and resubmit the preflight baseline source to restore the prior local analysis.

Allowed subtask statuses: `Not Started`, `In Progress`, `Blocked`, `Complete`, or `N/A — <specific reason>`.

| ID | Objective or deliverable | Included scope or target | Completion criterion | Expected evidence | Status | Actual evidence |
| --- | --- | --- | --- | --- | --- | --- |
| T-1 | Confirm authority, baseline, source identity, scanner availability, and secure token availability. | Local SonarQube project `koduck`; source `31328ab`; existing analysis baseline. | OCR is Accepted; dashboard baseline and source ancestry are captured; scanner is available; only non-empty scanner-process token availability is confirmed. | Approval metadata and non-sensitive preflight record. | Not Started | Pending — requires Accepted OCR. |
| T-2 | Submit exactly one approved-source local analysis. | Isolated clean worktree at `31328ab`; existing local project `koduck`. | Scanner submits the report and its compute-engine task completes successfully. | Scanner result, task identifier when retained, and processed source version. | Not Started | Pending — requires successful preflight. |
| T-3 | Verify scoped Reliability clearance or restore the captured baseline analysis. | Overall Open/Confirmed Reliability issue view and the nine former `validate.mjs` locations. | Zero active scoped rows; otherwise the captured baseline source is reanalyzed and becomes current. | Issue-view result, analysis identifier when retained, and recovery evidence when triggered. | Not Started | Pending — requires successful submission. |

## Eligibility [Required]

- [x] Uses an accepted architecture, pipeline, artifact contract, security boundary, and data boundary: `docs/adr/ADR-0014-validator-structural-parsing-reliability.md` authorizes the source correction, and this existing local scanner workflow changes neither a reusable artifact nor a security or data boundary.
- [x] Is reversible: capture the current analysis source revision before Execute; if a stop condition occurs after a new analysis is created, use an isolated worktree at that captured revision to resubmit the baseline through the same scanner workflow as recovery, then remove only the temporary worktree and scanner output.
- [x] Does not modify a Dockerfile, Makefile, CI, pipeline, artifact format, signing, credentials, deployment topology, API/schema/protocol, authentication, security policy, data lifecycle, dependency, provider, or irreversible behavior.
- [x] Has a defined preflight, success check, stop condition, and recovery path.
- [x] Contains no secret, credential, private endpoint, or sensitive user data. The token is neither recorded nor displayed in this OCR, commands, logs, or evidence.
- [x] N/A — no automatic-review mechanism is configured for this local, non-pushed operational input. Review is not treated as approval or execution evidence.

## Core Runbook And Evidence [Required]

### Preflight [Required]

**Planned action and criterion**: After this OCR is Accepted, confirm the local dashboard project key `koduck`, capture its current analysis source revision and the scoped Reliability baseline, confirm `31328ab` is clean in a detached worktree and includes `f2cc310`, confirm `sonar-scanner` is available, and confirm only that `KODUCK_SONAR_TOKEN` is non-empty for scanner-process setup. Do not print, persist, or transmit the token value.

**Actual result and stable evidence**: Pending — execute only after this OCR is Accepted.

### Execute [Required]

**Planned action**: From a clean detached worktree at `31328ab`, invoke `sonar-scanner` exactly once against the existing local project with `SONAR_HOST_URL=http://localhost:9000`, project key `koduck`, source revision `31328ab`, external disposable scanner working directory, excluded dependency/generated paths, and quality-gate wait enabled. Supply `KODUCK_SONAR_TOKEN` only by assigning it to the scanner process's `SONAR_TOKEN` environment variable; do not enable debug output, log the environment, or retry automatically.

**Actual result and stable evidence**: Pending — execute only after successful preflight.

### Verify [Required]

**Success criterion**: The scanner reports successful analysis submission and compute-engine completion, and the resulting overall Open/Confirmed Reliability view has zero active scoped rows in `tools/governance-validator/validate.mjs` for all nine ADR-0014 findings. Record only non-sensitive scanner, task, source-version, and issue-view evidence; a failed project Quality Gate is an observation, not a failure of this scoped criterion.

**Actual result and stable evidence**: Pending — verify only after the submitted task reaches a terminal state.

### Stop and Recovery [Required]

**Stop condition**: Stop if the OCR is not Accepted, the target project or baseline source cannot be confirmed, `31328ab` is not clean or lacks `f2cc310`, the scanner or token is unavailable, the scanner cannot submit a report, the compute-engine task fails, or any scoped target finding remains active.

**Recovery action**: If a new analysis exists after a stop condition, create an isolated temporary worktree at the captured baseline source revision and run the same local scanner workflow there to restore that baseline as current. Do not delete analyses or alter source, token, profile, project settings, issue state, quality gate, or scanner configuration.

**Recovery verification**: Confirm that the captured baseline source revision is again the current local project analysis and that its scoped target findings match the preflight baseline.

**Actual result and stable evidence**: Pending — if recovery is not triggered, record `Not triggered` with the successful task and scoped-result evidence.

## Conditional Extensions [Conditionally Required — production, multi-environment, phased, user/downstream/SLO impact, or stated change-window operation]

N/A — this is one local analysis with no production, multi-environment, phased, user/downstream, SLO, or change-window impact.

## Closure [Required]

- **Final result**: Not started — the proposed operation awaits acceptance.
- **Authorization review**: Pass — @linhai approval at 2026-09-01T11:03:13+08:00 precedes Execute.
- **Subtask and evidence review**: Not started — T-1 through T-3 await operational evidence.
- **Requirement-level review**: Pass — proposed-stage planned content is complete; terminal review follows execution or an evidenced stop.
- **Governance validation**: Pass — `npm run validate --prefix tools/governance-validator` reports `Governance validation passed.` for this draft.

## Supporting Notes [Optional]

The source change is limited to the one residual `validateSupersession` classifier reported by OCR-0011. This OCR does not alter project configuration or issue workflow; it either verifies the bounded source result or restores the captured baseline.

## Archival [Conditionally Required — Decision Status is retired or Implementation Status is final]

The record is Proposed and Not Started, so archival is inactive future-lifecycle guidance. If it reaches an archival-eligible state, move it under `docs/adr/ocr/archive/`, update its index row and every marker or reciprocal reference in the same change, retain `Superseded By: None` unless a replacement is identified, and confirm no active reference remains to the pre-archive path.

## Change Log [Required]

| Date | Change | Author |
| --- | --- | --- |
| 2026-09-01 | Drafted the reversible local SonarQube verification for ADR-0014 source correction `31328ab` after OCR-0011 restored its baseline. | @codex |
| 2026-09-01 | Governance validation passed before approval was requested. | @codex |
| 2026-09-01 | Accepted by @linhai with approval evidence `Approve` at 2026-09-01T11:03:13+08:00. | @linhai |
