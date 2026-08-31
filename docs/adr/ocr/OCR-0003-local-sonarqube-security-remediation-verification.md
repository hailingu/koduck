# OCR-0003: Local SonarQube Security Remediation Verification

## Metadata [Required]

- **Decision Status**: Accepted
- **Implementation Status**: Not Started
- **Date**: 2026-08-31
- **Author**: @codex
- **Decision Owner**: @linhai
- **Required Approver**: @linhai
- **Record Scope**: Project
- **Approver [Conditionally Required — Decision Status is or has been `Accepted`]**: @linhai
- **Approval Time [Conditionally Required — Decision Status is or has been `Accepted`]**: 2026-08-31T17:40:59+08:00
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
- **Operation Type**: Existing Runbook
- **Target Scope / Operation Owner**: Local SonarQube project `koduck` / @codex
- **Input Source or Version**: `2df31f9` — `docs(adr): record regex remediation evidence`; source remediation is `109c478580d8ec41bdf65b43b68cb8a0e90c14a8`
- **Expected Output or Target State**: A new local analysis for source input `2df31f9` completes successfully, and the open/confirmed Security issue filter for project `koduck` returns zero issues.
- **Docker Image Coordinates or Input Identity [Conditionally Required — container operation]**: N/A — this is not a container operation.
- **Kubernetes Target [Conditionally Required — Kubernetes operation]**: N/A — this is not a Kubernetes operation.
- **Actual immutable artifact [Conditionally Required — operation builds or consumes an artifact]**: N/A — the scanner submits an analysis report but does not build or consume a reusable artifact.
- **Dependencies**: Accepted `docs/adr/ADR-0007-linear-time-governance-path-recognition.md`; locally installed `sonar-scanner`; an already-provisioned project token supplied only through the scanner process environment; reachable local SonarQube service.
- **Related [Optional]**: Local SonarQube project `koduck`, Security issue view observed 2026-08-31.
- **Architecture Source [Conditionally Required — a governing ADR or ADD task applies]**: `docs/adr/ADR-0007-linear-time-governance-path-recognition.md`, subtask T-3
- **Supersedes [Conditionally Required — this OCR replaces another]**: N/A — no OCR is replaced
- **Superseded By [Conditionally Required — this OCR is replaced]**: None

## Requirement Level Legend [Required]

- **`[Required]`**: The section or field always applies and MUST remain present with complete, verifiable content. Use `None — <reason>` only when the template explicitly permits an empty result; never leave it blank.
- **`[Conditionally Required — <trigger>]`**: The section or field MUST be completed when its stated trigger applies. When the trigger does not apply, retain `N/A — <reason>` unless the template explicitly instructs removal or retention as inactive future-lifecycle guidance. A missing trigger assessment is incomplete content.
- **`[Optional]`**: The section may be removed without affecting acceptance, execution, completion, or verification. If retained, it MUST be accurate and complete; optional content MUST NOT substitute for required evidence.

Unlabeled fields inside a `[Required]` section are required.

Planned content is required before `Accepted`; actual-result fields remain `Pending` until their corresponding operation stage runs and are required before `Complete` or `Verified`.

## Task Definition [Required]

**Complete task outcome**: Submit one local SonarQube analysis of input `2df31f9` and verify that its Security issue filter reports zero open or confirmed findings for project `koduck`; otherwise stop and restore the pre-operation analysis baseline by deleting only the newly created analysis.

Allowed subtask statuses: `Not Started`, `In Progress`, `Blocked`, `Complete`, or `N/A — <specific reason>`.

| ID | Objective or deliverable | Included scope or target | Completion criterion | Expected evidence | Status | Actual evidence |
| --- | --- | --- | --- | --- | --- | --- |
| T-1 | Submit one source analysis to the existing local project. | Local SonarQube project `koduck`, source input `2df31f9`, main branch analysis. | Scanner exits 0 and reports a successful compute-engine task. | Scanner result, task identifier, and source revision. | Not Started | Pending |
| T-2 | Verify the remediated Security finding state. | Existing local project issue view, Security quality filter, open/confirmed statuses. | The filter returns exactly zero issues; no recovery action is required. | Issue-count result and target analysis timestamp or task identifier. | Not Started | Pending |

## Eligibility [Required]

- [x] Uses an accepted architecture, pipeline, artifact contract, security boundary, and data boundary: Accepted `docs/adr/ADR-0007-linear-time-governance-path-recognition.md` defines the source remediation and the established local project analysis pipeline; this operation neither produces a reusable artifact nor alters the token or data boundary.
- [x] Is reversible: before Execute, capture the current analysis identifier; if Execute or Verify fails after a new analysis is created, delete only that new analysis through the local SonarQube analysis-management operation and restore the captured baseline as current.
- [x] Does not modify a Dockerfile, Makefile, CI, pipeline, artifact format, signing, credentials, deployment topology, API/schema/protocol, authentication, security policy, data lifecycle, dependency, provider, or irreversible behavior.
- [x] Has a defined preflight, success check, stop condition, and recovery path.
- [x] Contains no secret, credential, private endpoint, or sensitive user data. The token is not recorded, displayed, or passed in this OCR.
- [x] N/A — no automatic-review mechanism is configured for this local, non-pushed operational input. Review is not treated as approval or execution evidence.

## Core Runbook And Evidence [Required]

### Preflight [Required]

**Planned action and criterion**: Confirm this OCR is Accepted before execution; confirm the local service exposes project `koduck` with the prior analysis identifier captured; confirm `sonar-scanner` is available; and confirm the scanner token is available only through its process environment without printing, storing, or transmitting it to any other destination.

**Actual result and stable evidence**: Pending

### Execute [Required]

**Planned action**: Run the installed scanner from the isolated task worktree against source input `2df31f9`, using the existing local project key and branch configuration. Supply the pre-provisioned token only through the scanner process environment. Do not log the token, scanner environment, or any copied credential.

**Actual result and stable evidence**: Pending

### Verify [Required]

**Success criterion**: The scanner reports successful analysis submission and compute-engine completion; the resulting project state has exactly zero Security issues with status Open or Confirmed. Record only non-sensitive scanner/task and issue-count evidence.

**Actual result and stable evidence**: Pending

### Stop and Recovery [Required]

**Stop condition**: Stop if the token is unavailable, the target project or baseline analysis cannot be confirmed, the scanner exits nonzero, the compute-engine task fails, or the target filter remains nonzero.

**Recovery action**: If a new analysis exists after a stop condition, delete only that analysis using the captured local baseline and record its identifier; otherwise make no project-state change. Do not alter source, token, profile, quality gate, or issue workflow state.

**Recovery verification**: Confirm that the captured pre-operation analysis is again the current local project analysis and that no new analysis remains.

**Actual result and stable evidence**: Pending — if recovery was not triggered, record `Not triggered` and the successful verification evidence.

## Conditional Extensions [Conditionally Required — production, multi-environment, phased, user/downstream/SLO impact, or stated change-window operation]

N/A — this is a single local analysis with no production, multi-environment, phased, user/downstream, SLO, or change-window impact.

## Closure [Required]

Allowed review statuses for Authorization review, Subtask and evidence review, and Requirement-level review are `Pass`, `Fail`, or `N/A — <specific reason>`.

- **Final result**: The approved operation has not yet executed; final result awaits its runbook evidence.
- **Authorization review**: Pass — Accepted approval metadata is complete and precedes execution.
- **Subtask and evidence review**: Execution-stage review is not yet applicable because T-1 and T-2 have not started.
- **Requirement-level review**: Pass — every acceptance-stage required and triggered field is complete; execution-stage evidence will be completed before terminal status.
- **Governance validation**: `npm run validate --prefix tools/governance-validator` must exit zero for the repository revision containing this record before execution.

## Supporting Notes [Optional]

The recovered baseline is the analysis immediately preceding Execute. It is captured only for local recovery and will be recorded without credentials if recovery is needed.

## Archival [Conditionally Required — Decision Status is retired or Implementation Status is final]

The record is Accepted and not archival-eligible. When this one-time operation becomes terminal, archive it under `docs/adr/ocr/archive/`, update its index row in the same change, and preserve any relevant recovery evidence.

## Change Log [Required]

| Date | Change | Author |
| --- | --- | --- |
| 2026-08-31 | Drafted the OCR for one local SonarQube remediation verification analysis. | @codex |
| 2026-08-31 | @linhai approved the unchanged planned operation. | @linhai |
