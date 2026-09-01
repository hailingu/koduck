# OCR-0010: Local SonarQube Relationship Validation Reliability Verification

## Metadata [Required]

- **Decision Status**: Accepted
- **Implementation Status**: In Progress
- **Date**: 2026-09-01
- **Author**: @codex
- **Decision Owner**: @linhai
- **Required Approver**: @linhai
- **Record Scope**: Project
- **Approver [Conditionally Required — Decision Status is or has been `Accepted`]**: @linhai
- **Approval Time [Conditionally Required — Decision Status is or has been `Accepted`]**: 2026-09-01T09:56:30+08:00
- **Approval Evidence [Conditionally Required — Decision Status is or has been `Accepted`]**: Approve
- **Rejector [Conditionally Required — Decision Status is `Rejected`]**: N/A — Decision Status is Accepted
- **Rejection Time [Conditionally Required — Decision Status is `Rejected`]**: N/A — Decision Status is Accepted
- **Rejection Evidence [Conditionally Required — Decision Status is `Rejected`]**: N/A — Decision Status is Accepted
- **Retired By [Conditionally Required — Decision Status is `Deprecated` or `Superseded`]**: N/A — record is not retired
- **Retirement Time [Conditionally Required — Decision Status is `Deprecated` or `Superseded`]**: N/A — record is not retired
- **Retirement Evidence [Conditionally Required — Decision Status is `Deprecated` or `Superseded`]**: N/A — record is not retired
- **Retirement Reason [Conditionally Required — Decision Status is `Deprecated` or `Superseded`]**: N/A — record is not retired
- **Blocked From [Conditionally Required — Implementation Status is `Blocked`]**: N/A — implementation is in progress and not blocked
- **Blocker And Evidence [Conditionally Required — Implementation Status is `Blocked`]**: N/A — implementation is in progress and not blocked
- **Blocker Owner [Conditionally Required — Implementation Status is `Blocked`]**: N/A — implementation is in progress and not blocked
- **Blocker Exit Or Recheck Criterion [Conditionally Required — Implementation Status is `Blocked`]**: N/A — implementation is in progress and not blocked
- **Operation Type**: Existing Runbook
- **Target Scope / Operation Owner**: Local SonarQube project `koduck` / @codex
- **Input Source or Version**: `f2cc310` — `fix(governance): harden relationship title validation`
- **Expected Output or Target State**: Exactly one successful local analysis of source `f2cc310`; the overall Open/Confirmed Reliability view has zero active rows for the former `relationship-validation.mjs` title-matcher and two literal-backtick replacement findings.
- **Docker Image Coordinates or Input Identity [Conditionally Required — container operation]**: N/A — this is not a container operation.
- **Kubernetes Target [Conditionally Required — Kubernetes operation]**: N/A — this is not a Kubernetes operation.
- **Actual immutable artifact [Conditionally Required — operation builds or consumes an artifact]**: N/A — the scanner submits a disposable analysis report and neither builds nor consumes a reusable artifact.
- **Dependencies**: Accepted `docs/adr/ADR-0013-relationship-validation-reliability.md`; locally installed SonarScanner CLI; reachable local SonarQube project `koduck`; the pre-provisioned `KODUCK_SONAR_TOKEN` supplied only through the scanner process environment; the baseline source revision captured during preflight for recovery.
- **Related [Optional]**: Local SonarQube project `koduck`; source remediation commit `f2cc310`; three scoped Reliability findings observed at `relationship-validation.mjs:L117`, `:L151`, and `:L152`.
- **Architecture Source [Conditionally Required — a governing ADR or ADD task applies]**: `docs/adr/ADR-0013-relationship-validation-reliability.md`, subtask T-3
- **Supersedes [Conditionally Required — this OCR replaces another]**: N/A — this is a new scoped verification and does not replace a prior OCR.
- **Superseded By [Conditionally Required — this OCR is replaced]**: None

## Requirement Level Legend [Required]

- **`[Required]`**: The section or field always applies and MUST remain present with complete, verifiable content. Use `None — <reason>` only when the template explicitly permits an empty result; never leave it blank.
- **`[Conditionally Required — <trigger>]`**: The section or field MUST be completed when its stated trigger applies. When the trigger does not apply, retain `N/A — <reason>` unless the template explicitly instructs removal or retention as inactive future-lifecycle guidance. A missing trigger assessment is incomplete content.
- **`[Optional]`**: The section may be removed without affecting acceptance, execution, completion, or verification. If retained, it MUST be accurate and complete; optional content MUST NOT substitute for required evidence.

Unlabeled fields inside a `[Required]` section are required.

Planned content is required before `Accepted`; actual-result fields remain `Pending` until their corresponding operation stage runs and are required before `Complete` or `Verified`.

## Task Definition [Required]

**Complete task outcome**: Submit exactly one local SonarQube analysis of source input `f2cc310` and verify zero active overall Reliability rows for the three scoped relationship-validation findings; if execution creates an analysis but the check fails, reanalyze the baseline source revision captured during preflight to restore it as the current analysis.

Allowed subtask statuses: `Not Started`, `In Progress`, `Blocked`, `Complete`, or `N/A — <specific reason>`.

| ID | Objective or deliverable | Included scope or target | Completion criterion | Expected evidence | Status | Actual evidence |
| --- | --- | --- | --- | --- | --- | --- |
| T-1 | Confirm authority, current baseline, scanner availability, and secure token availability. | Local SonarQube project `koduck`; source input `f2cc310`; baseline analysis source revision. | OCR is Accepted; baseline source is captured; scanner is available; only non-empty scanner-process token availability is confirmed. | Approval metadata and non-sensitive preflight record. | Complete | OCR approval preceded preflight; dashboard baseline is version `9ebf6a7` with 12 Reliability issues and all three scoped targets; scanner is 7.3.0.5189; the interactive profile confirmed only non-empty `KODUCK_SONAR_TOKEN` availability. |
| T-2 | Submit exactly one approved-source local analysis. | Temporary isolated worktree at `f2cc310`; existing local project `koduck`. | Scanner exits 0 and its compute-engine task completes successfully. | Scanner exit result, task identifier, and processed source version. | Not Started | Pending — execution awaits acceptance. |
| T-3 | Verify the scoped Reliability result or restore the captured baseline analysis. | Open/Confirmed overall Reliability issue view and the three scoped target findings. | Zero active scoped rows; otherwise the captured baseline source is reanalyzed and becomes current. | Issue-view result, analysis identifier, and recovery evidence when triggered. | Not Started | Pending — execution awaits acceptance. |

## Eligibility [Required]

- [x] Uses an accepted architecture, pipeline, artifact contract, security boundary, and data boundary: `docs/adr/ADR-0013-relationship-validation-reliability.md` authorizes the source correction; the existing local scanner workflow submits no reusable artifact and changes no security or data boundary.
- [x] Is reversible: capture the current analysis source revision before Execute; if a stop condition occurs after a new analysis is created, create an isolated temporary worktree at that captured revision and resubmit it as recovery through the same scanner workflow, then remove only the temporary worktree.
- [x] Does not modify a Dockerfile, Makefile, CI, pipeline, artifact format, signing, credentials, deployment topology, API/schema/protocol, authentication, security policy, data lifecycle, dependency, provider, or irreversible behavior.
- [x] Has a defined preflight, success check, stop condition, and recovery path.
- [x] Contains no secret, credential, private endpoint, or sensitive user data. The scanner token is not recorded, displayed, or passed in this OCR.
- [x] N/A — no automatic-review mechanism is configured for this local, non-pushed operational input. Review is not treated as approval or execution evidence.

## Core Runbook And Evidence [Required]

### Preflight [Required]

**Planned action and criterion**: Confirm this OCR is Accepted before execution; use the local SonarQube UI to confirm project `koduck` and capture its current analysis source revision and the three scoped Reliability rows; confirm the installed scanner is available; and confirm only that `KODUCK_SONAR_TOKEN` is non-empty at scanner-process setup without printing, storing, or transmitting its value.

**Actual result and stable evidence**: @linhai approval was recorded at 2026-09-01T09:56:30+08:00. The local dashboard showed current version `9ebf6a7` (last analysis 38 minutes earlier), 12 overall Reliability issues, and the three scoped Open targets at `relationship-validation.mjs` lines 117, 151, and 152. The scanner reported version 7.3.0.5189. The initial non-interactive shell had no token, while the interactive local profile confirmed only non-empty `KODUCK_SONAR_TOKEN` availability; no token value was displayed or recorded.

### Execute [Required]

**Planned action**: Create an isolated temporary Git worktree at `f2cc310`, then invoke the installed scanner once against the existing local project using its current project and branch configuration. Supply `KODUCK_SONAR_TOKEN` only through the scanner process environment. Do not log the token, scanner environment, or copied credential.

**Actual result and stable evidence**: Pending — execution awaits acceptance.

### Verify [Required]

**Success criterion**: The scanner reports successful analysis submission and compute-engine completion; the resulting overall Open/Confirmed Reliability view has exactly zero active target rows for the title matcher and both literal-backtick replacements. Record only non-sensitive scanner, task, and issue-view evidence.

**Actual result and stable evidence**: Pending — execution awaits acceptance.

### Stop and Recovery [Required]

**Stop condition**: Stop if the token is unavailable, the target project or baseline source revision cannot be confirmed, the scanner exits nonzero, the compute-engine task fails, or any scoped target finding remains active.

**Recovery action**: If a new analysis exists after a stop condition, create an isolated temporary Git worktree at the captured baseline source revision; run the same scanner workflow there with its matching project version; confirm the baseline analysis is current; and remove only the temporary worktree. Do not delete analyses or alter source, token, profile, quality gate, or issue workflow state.

**Recovery verification**: Confirm that the captured baseline source revision is again the current local project analysis and that its scoped target findings match the preflight baseline.

**Actual result and stable evidence**: Pending — if recovery is not triggered, record `Not triggered` with the successful scoped verification result.

## Conditional Extensions [Conditionally Required — production, multi-environment, phased, user/downstream/SLO impact, or stated change-window operation]

N/A — this is one local analysis with no production, multi-environment, phased, user/downstream, SLO, or change-window impact.

## Closure [Required]

- **Final result**: In Progress — preflight passed and one approved source analysis remains to be submitted.
- **Authorization review**: Pass — @linhai approval at 2026-09-01T09:56:30+08:00 precedes the first Execute action.
- **Subtask and evidence review**: N/A — operation has not executed.
- **Requirement-level review**: Pass — the Accepted OCR contains complete planned content and specific inactive-trigger assessments.
- **Governance validation**: Pass — `npm run validate` reported `Governance validation passed.` for the Accepted OCR revision.

## Supporting Notes [Optional]

The remaining nine Reliability findings in `tools/governance-validator/validate.mjs` are outside this operation and remain governed by a later ADR. The local source analysis is intentionally tied to the immutable remediation commit rather than the evolving task branch tip.

## Archival [Conditionally Required — Decision Status is retired or Implementation Status is final]

The record is Accepted and not final, so archival is inactive future-lifecycle guidance. If the operation reaches an archival-eligible state, move it under `docs/adr/ocr/archive/`, update its index row and every relevant reference in the same change, retain `Superseded By: None` unless a replacement is identified, and confirm no active reference remains to the pre-archive path.

## Change Log [Required]

| Date | Change | Author |
| --- | --- | --- |
| 2026-09-01 | Drafted the reversible local SonarQube verification for relationship-validation Reliability remediation commit `f2cc310`. | @codex |
| 2026-09-01 | Accepted by @linhai with approval evidence `Approve` at 2026-09-01T09:56:30+08:00. | @linhai |
| 2026-09-01 | Captured baseline version `9ebf6a7` with 12 Reliability issues and all three scoped targets; confirmed scanner 7.3.0.5189 and non-empty interactive-profile token availability without exposing its value. | @codex |
