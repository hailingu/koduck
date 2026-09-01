# OCR-0011: Local SonarQube Validator Structural Parsing Reliability Verification

## Metadata [Required]

- **Decision Status**: Accepted
- **Implementation Status**: Complete
- **Date**: 2026-09-01
- **Author**: @codex
- **Decision Owner**: @linhai
- **Required Approver**: @linhai
- **Record Scope**: Project
- **Approver [Conditionally Required — Decision Status is or has been `Accepted`]**: @linhai
- **Approval Time [Conditionally Required — Decision Status is or has been `Accepted`]**: 2026-09-01T10:46:28+08:00
- **Approval Evidence [Conditionally Required — Decision Status is or has been `Accepted`]**: Approve
- **Rejector [Conditionally Required — Decision Status is `Rejected`]**: N/A — Decision Status is Accepted.
- **Rejection Time [Conditionally Required — Decision Status is `Rejected`]**: N/A — Decision Status is Accepted.
- **Rejection Evidence [Conditionally Required — Decision Status is `Rejected`]**: N/A — Decision Status is Accepted.
- **Retired By [Conditionally Required — Decision Status is `Deprecated` or `Superseded`]**: N/A — record is not retired.
- **Retirement Time [Conditionally Required — Decision Status is `Deprecated` or `Superseded`]**: N/A — record is not retired.
- **Retirement Evidence [Conditionally Required — Decision Status is `Deprecated` or `Superseded`]**: N/A — record is not retired.
- **Retirement Reason [Conditionally Required — Decision Status is `Deprecated` or `Superseded`]**: N/A — record is not retired.
- **Blocked From [Conditionally Required — Implementation Status is `Blocked`]**: N/A — the stopped target analysis was recovered and this operation is complete.
- **Blocker And Evidence [Conditionally Required — Implementation Status is `Blocked`]**: N/A — the one remaining scoped row at `validate.mjs` L584 triggered the approved recovery, which restored the captured baseline.
- **Blocker Owner [Conditionally Required — Implementation Status is `Blocked`]**: N/A — no blocked operational work remains.
- **Blocker Exit Or Recheck Criterion [Conditionally Required — Implementation Status is `Blocked`]**: N/A — a later source correction and separately accepted OCR are required before another target analysis.
- **Operation Type**: Existing Runbook
- **Target Scope / Operation Owner**: Local SonarQube project `koduck` / @codex
- **Input Source or Version**: `f9d08d5` — validator structural-parsing source and focused-risk contract tests; it includes ADR-0013 source commit `f2cc310` through the locally integrated `dev` history.
- **Expected Output or Target State**: Exactly one successful local analysis of source `f9d08d5`; the overall Open/Confirmed Reliability view has zero active rows in `tools/governance-validator/validate.mjs` for the eight former structural-parser findings and the former literal-hyphen-normalization finding.
- **Docker Image Coordinates or Input Identity [Conditionally Required — container operation]**: N/A — this is not a container operation.
- **Kubernetes Target [Conditionally Required — Kubernetes operation]**: N/A — this is not a Kubernetes operation.
- **Actual immutable artifact [Conditionally Required — operation builds or consumes an artifact]**: N/A — the scanner submits a disposable analysis report and neither builds nor consumes a reusable artifact.
- **Dependencies**: Accepted `docs/adr/ADR-0014-validator-structural-parsing-reliability.md`; source commits `536cf5b` and `f9d08d5`; ADR-0013 source commit `f2cc310` in the input history; locally installed `sonar-scanner`; reachable local SonarQube project `koduck`; and the pre-provisioned `KODUCK_SONAR_TOKEN` supplied only to the scanner process environment.
- **Related [Optional]**: Local SonarQube project `koduck`; the nine Reliability findings originally observed in `tools/governance-validator/validate.mjs`; source evidence commit `3740c23`.
- **Architecture Source [Conditionally Required — a governing ADR or ADD task applies]**: `docs/adr/ADR-0014-validator-structural-parsing-reliability.md`, subtask T-3
- **Supersedes [Conditionally Required — this OCR replaces another]**: N/A — this is a new scoped verification and does not replace a prior OCR.
- **Superseded By [Conditionally Required — this OCR is replaced]**: None

## Requirement Level Legend [Required]

- **`[Required]`**: The section or field always applies and MUST remain present with complete, verifiable content. Use `None — <reason>` only when the template explicitly permits an empty result; never leave it blank.
- **`[Conditionally Required — <trigger>]`**: The section or field MUST be completed when its stated trigger applies. When the trigger does not apply, retain `N/A — <reason>` unless the template explicitly instructs removal or retention as inactive future-lifecycle guidance. A missing trigger assessment is incomplete content.
- **`[Optional]`**: The section may be removed without affecting acceptance, execution, completion, or verification. If retained, it MUST be accurate and complete; optional content MUST NOT substitute for required evidence.

Unlabeled fields inside a `[Required]` section are required.

Planned content is required before `Accepted`; actual-result fields remain `Pending` until their corresponding operation stage runs and are required before `Complete` or `Verified`.

## Task Definition [Required]

**Complete task outcome**: Submit exactly one local SonarQube analysis of immutable source `f9d08d5` and verify that no scoped `validate.mjs` Reliability row remains; otherwise stop and resubmit the preflight baseline source to restore the prior local analysis.

Allowed subtask statuses: `Not Started`, `In Progress`, `Blocked`, `Complete`, or `N/A — <specific reason>`.

| ID | Objective or deliverable | Included scope or target | Completion criterion | Expected evidence | Status | Actual evidence |
| --- | --- | --- | --- | --- | --- | --- |
| T-1 | Confirm authority, baseline, source identity, scanner availability, and secure token availability. | Local SonarQube project `koduck`; source `f9d08d5`; existing analysis baseline. | OCR is Accepted; dashboard baseline and source ancestry are captured; scanner is available; only non-empty scanner-process token availability is confirmed. | Approval metadata and non-sensitive preflight record. | Complete | OCR approval at 2026-09-01T10:46:28+08:00 preceded preflight. The existing `f2cc310` baseline matched OCR-0010 and showed nine scoped Reliability rows; `f9d08d5` contains `f2cc310`; scanner 7.3.0.5189 was available; the interactive profile confirmed only that `KODUCK_SONAR_TOKEN` was non-empty. |
| T-2 | Submit exactly one approved-source local analysis. | Isolated clean worktree at `f9d08d5`; existing local project `koduck`. | Scanner exits 0 and its compute-engine task completes successfully. | Scanner exit result, task identifier, and processed source version. | N/A — quality-gate wait returned 3 after processing | At 10:52 the scanner uploaded and the server processed source `f9d08d58bea67a28628a7d9967a2860a05939895`. Its quality-gate wait returned 3 solely because the project gate was Failed; this expected non-scoped outcome triggered T-3 verification. The disposable external report output was removed after recovery, so no task identifier was retained. |
| T-3 | Verify scoped Reliability clearance or restore the captured baseline analysis. | Overall Open/Confirmed Reliability issue view and the nine former `validate.mjs` locations. | Zero active scoped rows; otherwise the captured baseline source is reanalyzed and becomes current. | Issue-view result, analysis identifier, and recovery evidence when triggered. | Complete | The target view had one remaining scoped Reliability row at `validate.mjs` L584, so recovery was triggered. The `f2cc310` recovery analysis completed successfully at 10:53; the dashboard now identifies version `f2cc310` and the captured nine Reliability rows. |

## Eligibility [Required]

- [x] Uses an accepted architecture, pipeline, artifact contract, security boundary, and data boundary: `docs/adr/ADR-0014-validator-structural-parsing-reliability.md` authorizes the source correction, and this existing local scanner workflow changes neither a reusable artifact nor a security or data boundary.
- [x] Is reversible: capture the current analysis source revision before Execute; if a stop condition occurs after a new analysis is created, use an isolated worktree at that captured revision to resubmit the baseline through the same scanner workflow as recovery, then remove only the temporary worktree and scanner output.
- [x] Does not modify a Dockerfile, Makefile, CI, pipeline, artifact format, signing, credentials, deployment topology, API/schema/protocol, authentication, security policy, data lifecycle, dependency, provider, or irreversible behavior.
- [x] Has a defined preflight, success check, stop condition, and recovery path.
- [x] Contains no secret, credential, private endpoint, or sensitive user data. The token is neither recorded nor displayed in this OCR, commands, logs, or evidence.
- [x] N/A — no automatic-review mechanism is configured for this local, non-pushed operational input. Review is not treated as approval or execution evidence.

## Core Runbook And Evidence [Required]

### Preflight [Required]

**Planned action and criterion**: After this OCR is Accepted, confirm the local dashboard project key `koduck`, capture its current analysis source revision and the scoped Reliability baseline, confirm `f9d08d5` is clean in a detached worktree and includes `f2cc310`, confirm `sonar-scanner` is available, and confirm only that `KODUCK_SONAR_TOKEN` is non-empty for scanner-process setup. Do not print, persist, or transmit the token value.

**Actual result and stable evidence**: @linhai approval at 2026-09-01T10:46:28+08:00 preceded preflight. The local Reliability view showed the captured `f2cc310` baseline with nine scoped `validate.mjs` rows. `f9d08d5` was checked out cleanly in a detached worktree and includes `f2cc310`; scanner 7.3.0.5189 was available. The interactive profile confirmed only non-empty `KODUCK_SONAR_TOKEN` availability; its value was neither displayed nor retained.

### Execute [Required]

**Planned action**: From a clean detached worktree at `f9d08d5`, invoke `sonar-scanner` exactly once against the existing local project with `SONAR_HOST_URL=http://localhost:9000`, project key `koduck`, source revision `f9d08d5`, external disposable scanner working directory, excluded dependency/generated paths, and quality-gate wait enabled. Supply `KODUCK_SONAR_TOKEN` only by assigning it to the scanner process's `SONAR_TOKEN` environment variable; do not enable debug output, log the environment, or retry automatically.

**Actual result and stable evidence**: The one approved-source scan uploaded the `f9d08d58bea67a28628a7d9967a2860a05939895` report at 10:52 and the dashboard processed it. Quality-gate waiting returned code 3 because the project Quality Gate was Failed; no scan retry occurred. The external disposable report directory was removed after recovery, so no compute-engine task identifier was retained.

### Verify [Required]

**Success criterion**: The scanner reports successful analysis submission and compute-engine completion, and the resulting overall Open/Confirmed Reliability view has zero active scoped rows in `tools/governance-validator/validate.mjs` for all nine ADR-0014 findings. Record only non-sensitive scanner, task, source-version, and issue-view evidence; a failed project Quality Gate is an observation, not a failure of this scoped criterion.

**Actual result and stable evidence**: Fail for the target source: the overall Reliability view reduced to one open scoped row, `tools/governance-validator/validate.mjs` L584. This invokes the OCR stop condition; it is not a successful scoped clearance.

### Stop and Recovery [Required]

**Stop condition**: Stop if the OCR is not Accepted, the target project or baseline source cannot be confirmed, `f9d08d5` is not clean or lacks `f2cc310`, the scanner or token is unavailable, the scanner exits nonzero, the compute-engine task fails, or any scoped target finding remains active.

**Recovery action**: If a new analysis exists after a stop condition, create an isolated temporary worktree at the captured baseline source revision and run the same local scanner workflow there to restore that baseline as current. Do not delete analyses or alter source, token, profile, project settings, issue state, quality gate, or scanner configuration.

**Recovery verification**: Confirm that the captured baseline source revision is again the current local project analysis and that its scoped target findings match the preflight baseline.

**Actual result and stable evidence**: Triggered and completed. The isolated `f2cc310` recovery scan uploaded and completed successfully at 10:53 with a Passed Quality Gate. The local dashboard then showed version `f2cc310`, nine overall open Reliability rows, and the original nine scoped `validate.mjs` rows. Both detached worktrees and both external disposable scanner-output directories were removed; no analysis, issue state, source, token, profile, or project setting was deleted or changed outside the approved scans.

## Conditional Extensions [Conditionally Required — production, multi-environment, phased, user/downstream/SLO impact, or stated change-window operation]

N/A — this is one local analysis with no production, multi-environment, phased, user/downstream, SLO, or change-window impact.

## Closure [Required]

- **Final result**: Complete — the target analysis stopped on one residual scoped row and the captured `f2cc310` baseline was successfully restored.
- **Authorization review**: Pass — @linhai accepted this OCR before execution.
- **Subtask and evidence review**: Pass — T-1 is complete, T-2 has its specific Quality-Gate return-code disposition, and T-3 completed the required recovery and cleanup.
- **Requirement-level review**: Pass — all required terminal evidence is recorded and every conditional trigger has a specific disposition.
- **Governance validation**: Pass — `npm run validate --prefix tools/governance-validator` reports `Governance validation passed.` after terminal evidence and archival updates.

## Supporting Notes [Optional]

This OCR does not alter the SonarQube project configuration or finding workflow. It verifies only the nine ADR-0014 targets; the previous ADR-0013 source correction is an input prerequisite already integrated into `dev`, not an operational target of this record.

## Archival [Conditionally Required — Decision Status is retired or Implementation Status is final]

The operation is terminal and this record is archived under `docs/adr/ocr/archive/` in the same change as its `Complete` status and index-path update. `Superseded By` remains `None`; active references now use the archived path.

## Change Log [Required]

| Date | Change | Author |
| --- | --- | --- |
| 2026-09-01 | Drafted the reversible local SonarQube verification for ADR-0014 source input `f9d08d5`. | @codex |
| 2026-09-01 | Accepted by @linhai with approval evidence `Approve` at 2026-09-01T10:46:28+08:00. | @linhai |
| 2026-09-01 | Submitted source `f9d08d5`; one scoped L584 Reliability row remained, so the approved `f2cc310` recovery restored the prior nine-row local baseline and removed disposable worktrees and scanner output. | @codex |
