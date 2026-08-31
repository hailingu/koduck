<!-- markdownlint-disable MD041 -->

---

# ADR-0006: One-Time Local SonarQube Baseline Analysis

## Metadata [Required]

- **Decision Status**: Deprecated
- **Implementation Status**: Verified
- **Date**: 2026-08-31
- **Author**: @codex
- **Decision Owner**: @linhai
- **Required Approver**: @linhai
- **Record Scope**: Project
- **Approver [Conditionally Required — Decision Status is or has been `Accepted`]**: @linhai
- **Approval Time [Conditionally Required — Decision Status is or has been `Accepted`]**: 2026-08-31T16:30:53+08:00
- **Approval Evidence [Conditionally Required — Decision Status is or has been `Accepted`]**: Approve
- **Rejector [Conditionally Required — Decision Status is `Rejected`]**: N/A — Decision Status is `Proposed`
- **Rejection Time [Conditionally Required — Decision Status is `Rejected`]**: N/A — Decision Status is `Proposed`
- **Rejection Evidence [Conditionally Required — Decision Status is `Rejected`]**: N/A — Decision Status is `Proposed`
- **Retired By [Conditionally Required — Decision Status is `Deprecated` or `Superseded`]**: @linhai
- **Retirement Time [Conditionally Required — Decision Status is `Deprecated` or `Superseded`]**: 2026-08-31T22:01:15+08:00
- **Retirement Evidence [Conditionally Required — Decision Status is `Deprecated` or `Superseded`]**: Deprecate
- **Retirement Reason [Conditionally Required — Decision Status is `Deprecated` or `Superseded`]**: The completed one-time baseline is retained as historical evidence; the Decision Owner directed archival before further Reliability remediation.
- **Blocked From [Conditionally Required — Implementation Status is `Blocked`]**: N/A — Implementation Status is `Verified`
- **Blocker And Evidence [Conditionally Required — Implementation Status is `Blocked`]**: N/A — Implementation Status is `Verified`
- **Blocker Owner [Conditionally Required — Implementation Status is `Blocked`]**: N/A — Implementation Status is `Verified`
- **Blocker Exit Or Recheck Criterion [Conditionally Required — Implementation Status is `Blocked`]**: N/A — Implementation Status is `Verified`
- **Architecture Source [Conditionally Required — product demand]**: N/A — this repository-owner-requested local verification strategy is not derived from Trello product demand
- **Supersedes [Conditionally Required — this ADR replaces another]**: None
- **Superseded By [Conditionally Required — this ADR is replaced]**: None

## Requirement Level Legend [Required]

- **`[Required]`**: The section or field always applies and MUST remain present
  with complete, verifiable content. Use `None — <reason>` only when the
  template explicitly permits an empty result; never leave it blank.
- **`[Conditionally Required — <trigger>]`**: The section or field MUST be
  completed when its stated trigger applies. When the trigger does not apply,
  retain `N/A — <reason>` unless the template explicitly instructs removal or
  retention as inactive future-lifecycle guidance. A missing trigger assessment
  is incomplete content.
- **`[Optional]`**: The section may be removed without affecting acceptance,
  execution, completion, or verification. If retained, it MUST be accurate and
  complete; optional content MUST NOT substitute for required evidence.

Unlabeled fields inside a `[Required]` section are required.

## Context And Problem Statement [Required]

The local SonarQube Community Build at `http://localhost:9000` now contains a
public project with display name `Koduck`, project key `koduck`, and main branch
`main`, but that branch has never been analyzed. The repository contains no
SonarQube configuration, scanner script, CI integration, accepted analysis
runbook, or credential contract. Running the installed SonarScanner would write
source-analysis data to a running system and therefore is not Disposable
Verification Execution.

This decision establishes one bounded, manually invoked baseline analysis of
the immutable local `dev` commit
`8a9c5c8b1c2d1c8faa157f80374f51faf2ebe044`. It does not establish a recurring
pipeline or add repository configuration. The scanner consumes a pre-existing
project-scoped analysis token only from the `SONAR_TOKEN` process environment;
the token value must never enter a command argument, repository file, log,
captured evidence, browser output, or task response.

## Scope [Required]

In scope:

- Analyze exactly commit `8a9c5c8b1c2d1c8faa157f80374f51faf2ebe044`
  from an isolated clean worktree with SonarScanner CLI `7.3.0.5189` against
  local SonarQube project key `koduck`.
- Wait up to 300 seconds for server-side computation, record whether the
  Quality Gate is passed or failed, and verify that the `main` branch dashboard
  represents the declared commit.
- Delete scanner working files and the disposable scan worktree after evidence
  is captured.

Out of scope:

- Adding `sonar-project.properties`, a scanner script, dependency, Makefile
  target, CI workflow, required check, quality profile, quality gate, project
  permission, or recurring analysis schedule.
- Generating, rotating, revoking, printing, persisting, or otherwise changing
  SonarQube credentials or authentication policy.
- Changing source, tests, documentation other than this decision record and
  its index row, project settings, the existing `Koduck` project identity, or
  any finding produced by the analysis.
- Treating a failed Quality Gate as an execution failure or changing code to
  make the gate pass; the requested outcome is the first measured baseline.

## Tensions, Constraints, And Open Questions [Required]

### Identified Tensions [Conditionally Required — competing goals or trade-offs exist]

| ID | Tension | Impact | Decision |
| --- | --- | --- | --- |
| TN-1 | Obtain a useful first baseline without silently establishing a permanent build or CI contract. | Persisting scanner configuration would create an unapproved recurring verification strategy. | Pass all one-time properties on the scanner invocation and retain no repository configuration. |
| TN-2 | Authenticate the scan without exposing or creating a credential. | A token in a file, argument, log, browser output, or task response could leak persistent access. | Require an existing project-scoped token through `SONAR_TOKEN`, verify only that it is non-empty, and never echo or persist it. |

### Constraints [Required]

- This ADR must be `Accepted` by an eligible non-author approver before the
  scanner contacts the analysis endpoint.
- The input must remain exactly commit
  `8a9c5c8b1c2d1c8faa157f80374f51faf2ebe044`; a changed `dev` ref or dirty scan
  worktree stops execution.
- The installed scanner must report version `7.3.0.5189`, and SonarQube must
  report project key `koduck` with an unanalyzed `main` branch before execution.
- Authentication must use a pre-existing project-scoped analysis token supplied
  through `SONAR_TOKEN`; token creation or credential changes require a separate
  authorized decision and are not part of this task.
- The scanner must use `SONAR_HOST_URL=http://localhost:9000`, must not use a
  proxy or remote SonarQube host, and must not run in debug mode.
- No source, repository configuration, project settings, finding state, quality
  profile, or quality gate may be modified as part of the analysis.

### Open Questions [Conditionally Required — material questions exist or were resolved during drafting]

None — no material questions remain. Token availability is an execution
precondition with an explicit stop condition, not an unresolved decision.

## Decision Drivers [Required]

1. **Bounded external effect**: Only one immutable commit is analyzed, with no
   recurring automation or repository configuration.
2. **Credential containment**: The token remains outside repository content,
   command arguments, logs, and captured evidence.
3. **Decidable result**: Server computation must reach a terminal state and the
   dashboard must expose the analyzed branch and Quality Gate result.
4. **Worktree safety**: The user's existing uncommitted ADR changes in the
   original `dev` worktree must remain untouched and excluded from the scan.

## Options Considered [Required]

### Option: One-time local CLI analysis of an immutable commit

Create a detached clean worktree for the exact input commit, invoke the
installed scanner with all non-secret properties on the command line and the
token only in `SONAR_TOKEN`, wait for server computation, record the dashboard
result, and remove disposable local output.

Pros:

- Produces the requested first baseline without modifying source or pipeline
  configuration.
- Binds the result to one exact commit and contains local scanner output.

Cons:

- Does not provide repeatable CI coverage for later commits.
- Requires the repository owner to supply an existing token securely at run
  time.

### Option: Add recurring GitHub Actions analysis

Add repository scanner configuration, a CI workflow, secret management, and a
required or advisory check.

Pros:

- Automatically analyzes later revisions.

Cons:

- Introduces a broader pipeline, credential, and branch-governance decision
  outside the requested first local check.
- Requires source/configuration implementation and review beyond this task.

### Option: Do not run an analysis

Leave the newly created project without a baseline.

Pros:

- Creates no external analysis data and requires no credential.

Cons:

- Does not satisfy the request and leaves SonarQube without measurements.

## Decision [Required]

**Selected option**: One-time local CLI analysis of an immutable commit.

**Rationale**: This option delivers the requested baseline while keeping the
external write, credential exposure surface, input revision, execution time,
and retained local output explicitly bounded. It avoids deciding a recurring
pipeline or repository configuration contract before those broader concerns
are requested and designed.

### Consequences [Required]

Positive:

- SonarQube receives one traceable baseline for project `koduck` and main
  branch `main`.
- The user's dirty original worktree and uncommitted ADR edits remain outside
  both the scanner input and all cleanup actions.
- A Quality Gate failure remains visible evidence instead of triggering an
  unapproved code change.

Negative:

- The result becomes stale as soon as later commits are made.
- SonarQube retains analysis data and findings after the local scanner process
  and disposable worktree are removed.
- The operation cannot proceed until a suitable token already exists and is
  supplied securely.

Mitigations:

- Bind the analysis metadata and evidence to the exact full commit SHA.
- Record Quality Gate state as an observation, not a promise that the baseline
  passes.
- Stop before analysis when authorization, input, target, scanner version, or
  token preconditions are not exact; never generate or persist a token.

## Implementation Plan [Required]

**Complete task outcome**: Local SonarQube project `koduck` contains one
completed baseline analysis for main branch `main` at commit
`8a9c5c8b1c2d1c8faa157f80374f51faf2ebe044`, the terminal Quality Gate result is
recorded without altering findings or settings, and all disposable scanner
output and the isolated scan worktree are removed without touching the user's
original worktree.

**Primary implementation boundary**: Local SonarQube analysis submission and
result-verification boundary; no repository source or configuration boundary is
modified.

Allowed subtask statuses: `Not Started`, `In Progress`, `Blocked`, `Complete`,
or `N/A — <specific reason>`.

| ID | Objective or deliverable | Included scope | Status | Actual implementation evidence |
| --- | --- | --- | --- | --- |
| T-1 | Submit the one-time baseline analysis for the exact clean commit. | Detached clean worktree, SonarScanner CLI `7.3.0.5189`, local project `koduck`, environment-only token, and bounded server wait. | Complete | After governance validation, the paused process received non-secret signal `RUN_APPROVED`. SonarScanner `7.3.0.5189` indexed 212 files in JavaScript, JSON, Rust, and YAML, uploaded one report for SCM revision `8a9c5c8b1c2d1c8faa157f80374f51faf2ebe044`, and exited 0. Compute task `a0c652a0-e42c-4dd2-b372-9680d54ca327` completed within the 300-second bound and reported `QUALITY GATE STATUS: PASSED`. |
| T-2 | Verify and report the terminal baseline, then remove disposable local output. | Compute-task result, main-branch dashboard, Quality Gate result, scanner working directory, and detached scan worktree. | Complete | The authenticated dashboard showed `Koduck`, version `8a9c5c8b1c2d1c8faa157f80374f51faf2ebe044`, `First analysis`, and Quality Gate `Passed` at Aug 31, 2026, 5:02 PM. All five exact temporary paths were removed; the original worktree retained only its two pre-existing ADR modifications. |

**Affected paths**: `docs/adr/archive/ADR-0006-one-time-local-sonarqube-baseline-analysis.md`;
`docs/adr/INDEX.md`; disposable paths under `/private/tmp` created only after
approval and removed before completion; local SonarQube project key `koduck`.

### Stable Implementation Touchpoints [Conditionally Required — source or configuration implementation]

N/A — this decision performs a bounded external analysis and changes no
repository source or configuration.

**Migration and rollback strategy [Conditionally Required — this replaces or
changes existing behavior]**: N/A — no existing repository behavior or
configuration is replaced. Stop before submission on any preflight mismatch.
After successful submission, preserve the truthful baseline even when the
Quality Gate fails; deleting analysis data would destroy requested evidence
and is outside this decision.

### Engineering Exceptions [Conditionally Required — an engineering rule is exceeded or waived]

N/A — no maintained source or configuration is implemented or exempted.

## Contract-To-Check Traceability [Conditionally Required — source or configuration implementation]

N/A — no repository source or configuration contract is implemented.

## Risk Coverage Matrix [Conditionally Required — source or configuration implementation]

N/A — no repository source or configuration is implemented; operational
timeout, credential, input-revision, and cleanup risks are covered directly by
AC-1 through AC-4.

## Acceptance Checks [Required]

| Check ID | Subtask | Binary acceptance point | Preconditions or input | Verification method | Exact expected result | Expected evidence | Status | Actual result and evidence |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| AC-1 | T-1 | Authorization, target, input, scanner, and credential containment preconditions are exact before submission. | This ADR is `Accepted`; `dev` and the detached worktree both resolve to `8a9c5c8b1c2d1c8faa157f80374f51faf2ebe044`; scan worktree status is empty; scanner is `7.3.0.5189`; project `koduck` exists with unanalyzed `main`; `SONAR_TOKEN` is non-empty. | Inspect ADR metadata; run read-only Git, scanner-version, environment-presence, and authenticated SonarQube project/status checks without printing the token. | Every stated value matches exactly; no token value appears in command arguments or captured output; otherwise no analysis command runs. | Timestamped preflight command summary with exact non-secret values and pass/fail state. | Pass | `@linhai` approved before Execute; browser session confirmed `koduck` redirected to onboarding with unanalyzed `main`; detached worktree was clean at the exact full SHA; scanner reported `7.3.0.5189`; the newly generated raw token passed format containment in process memory. The process paused until post-resume governance validation passed, and no credential value appeared in a command argument or captured output. |
| AC-2 | T-1 | The scanner submits exactly one analysis of the declared commit to local project `koduck` and the compute task terminates within 300 seconds. | AC-1 passes and no stop condition is present. | Run SonarScanner once with `SONAR_HOST_URL=http://localhost:9000`, exact project and revision properties, excluded generated/dependency paths, external scanner working directory, and `sonar.qualitygate.wait=true`; never enable debug output or retry a submission automatically. | Scanner output identifies project key `koduck`, server `http://localhost:9000`, revision `8a9c5c8b1c2d1c8faa157f80374f51faf2ebe044`, one compute task, and a terminal server result within 300 seconds. A passed or failed Quality Gate is accepted as the truthful baseline; authentication, transport, scanner, or compute failure is `Fail`. | Redacted scanner summary, exit status, compute-task identifier, terminal task state, analysis identifier, and Quality Gate state. | Pass | The authorized final invocation communicated with SonarQube `26.8.0.126808`, indexed 212 files, logged exact SCM revision `8a9c5c8b1c2d1c8faa157f80374f51faf2ebe044`, uploaded one report, and waited for processing. It reported `QUALITY GATE STATUS: PASSED`, `EXECUTION SUCCESS`, exit 0, total time 38.842 seconds, and compute task `a0c652a0-e42c-4dd2-b372-9680d54ca327`. Earlier rejected attempts stopped before report creation and were not submissions. |
| AC-3 | T-2 | The SonarQube dashboard represents the exact analyzed main branch and exposes a terminal Quality Gate result. | AC-2 reaches a terminal successful compute state. | Inspect the authenticated project dashboard and project-analysis APIs for project key `koduck` without changing settings or findings. | Main branch `main` has an analysis whose revision is the declared full SHA; Quality Gate is exactly passed or failed; the dashboard no longer says the main branch is unanalyzed. | Dashboard URL, analysis timestamp, revision, branch, Quality Gate state, and read-only API/browser inspection summary. | Pass | Authenticated inspection of `http://localhost:9000/dashboard?id=koduck&codeScope=overall` showed `Koduck`, version `8a9c5c8b1c2d1c8faa157f80374f51faf2ebe044`, `First analysis` at Aug 31, 2026, 5:02 PM, and Quality Gate `Passed`; the dashboard no longer redirected to onboarding. It reported 92 issues, 0.0% coverage, and 7.9% duplication. |
| AC-4 | T-2 | Disposable scanner output is removed and the original user worktree is unchanged by execution or cleanup. | AC-2 and AC-3 evidence is captured, or execution stopped before submission. | Remove only the exact recorded disposable scanner directory and detached scan worktree; compare original worktree status to the recorded preflight status. | Disposable paths do not exist; original worktree still contains exactly its preflight user-owned modifications and no scanner output or Sonar configuration; no token was printed or persisted. | Cleanup commands, path-absence checks, before/after status comparison, and credential-containment review. | Pass | Before deletion, the scanner log contained no `sqp_`, `SONAR_TOKEN`, or `sonar.token` marker and the detached worktree was clean. The scanner work directory, `report-task.txt`, scanner log, temporary runner, and detached scan worktree were then removed and confirmed absent. The original worktree still showed only the pre-existing modifications to `docs/adr/ADR-0001-provider-neutral-turn-kernel.md` and `docs/adr/ADR-0003-default-deny-tool-approval-execution-boundary.md`; no Sonar configuration was added. |

Allowed final check statuses are `Pass`, `Fail`, or `N/A — <specific reason>`.
`Fail` blocks completion. `N/A` is valid only when the check's stated trigger or
precondition demonstrably does not apply.

## Completion Checklist [Required]

| ID | Item | Completion Criterion | Expected Evidence | Status | Actual Evidence |
| --- | --- | --- | --- | --- | --- |
| A-1 | ADR approved | An eligible non-author approver, approval time, and exact `Approval Evidence: Approve` are recorded. | ADR metadata | Complete | `@linhai` responded with exact `Approve` at `2026-08-31T16:30:53+08:00` in the active task context that identifies only this ADR; the approver is not the drafting agent. |
| A-2 | Complete task delivered | T-1 and T-2 are complete and AC-1 through AC-4 are `Pass`. | Implementation Plan and Acceptance Checks | Complete | T-1 and T-2 are `Complete`; AC-1 through AC-4 are `Pass`; scanner, compute-task, dashboard, credential-containment, worktree-preservation, and cleanup evidence satisfy the complete task outcome. |
| A-3 | Reciprocal ADD link synchronized, when applicable | Product-demand handoff does not apply. | Architecture Source metadata | N/A — this repository-owner-requested verification is not derived from product demand | Architecture Source records the specific N/A reason. |
| A-4 | Requirement levels satisfied | Every required and triggered field is complete or has a specific N/A reason for the current stage. | Structured document review | Complete | Structured review found every required and triggered field complete for `Verified`; non-applicable source/configuration and retirement sections retain specific reasons or inactive guidance. |
| A-5 | Acceptance checks are decidable | AC-1 through AC-4 each name one subtask, exact input, deterministic method, observable result, and evidence. | Structured acceptance-check review | Complete | AC-1 through AC-4 each identify one subtask, exact preconditions, a deterministic method, a binary expected result, and stable actual evidence. |
| A-6 | Engineering exceptions governed, when applicable | No rule is exceeded or waived. | Engineering Exceptions subsection | N/A — no maintained source or configuration is implemented | The subsection records the specific N/A reason. |
| A-7 | Contract and baseline risks covered, when applicable | Source/configuration traceability and the five-row Risk Coverage Matrix do not apply; operational risks are explicitly checked. | Conditional sections and AC-1 through AC-4 | N/A — no source or configuration implementation | The conditional sections state why they do not apply, and AC-1 through AC-4 cover input, timeout, trust, result, and cleanup. |
| A-8 | Governance validation passed | The independent validator reports no lifecycle, required-section, template-field, index, or cross-reference error. | `npm run validate --prefix tools/governance-validator` output | Complete | `npm run validate --prefix tools/governance-validator` passed after completion evidence and the `Accepted / Verified` index transition were recorded. |

## Archival [Conditionally Required — Decision Status is `Rejected`, or Decision Status is `Deprecated` or `Superseded` and Implementation Status is final]

The record is `Deprecated / Verified` and is archived under `docs/adr/archive/`.

- [x] Moved this file to `archive/ADR-0006-one-time-local-sonarqube-baseline-analysis.md` under `docs/adr/`.
- [x] No governed-file marker cites this record because the one-time analysis did not modify repository source.
- [x] No record supersedes this one; `Superseded By: None` is retained.
- [x] Updated this record's single row in `docs/adr/INDEX.md`.
- [x] Confirmed no active record or governed marker cites the pre-archive path.

## Change Log [Required]

| Date | Change | Author |
| --- | --- | --- |
| 2026-08-31 | Proposed one bounded local SonarQube baseline analysis for immutable `dev` commit `8a9c5c8b1c2d1c8faa157f80374f51faf2ebe044`, with environment-only pre-existing token use, a finite server wait, truthful Quality Gate reporting, and disposable-worktree cleanup. | @codex |
| 2026-08-31 | Recorded `@linhai`'s exact `Approve` response at `2026-08-31T16:30:53+08:00`, set the ADR to `Accepted / In Progress`, and began the approved preflight without using the token previously exposed in chat. | @codex |
| 2026-08-31 | At `2026-08-31T16:43:01+08:00`, recorded the operation as `Blocked` from `In Progress`: authenticated browser preflight still showed no analysis, but the clipboard held an assignment with a `SONAR_TOKEN=` prefix and the scanner returned HTTP 401 before submission. No `report-task.txt` or SonarQube analysis was created. | @codex |
| 2026-08-31 | At `2026-08-31T16:56:51+08:00`, satisfied the blocker exit criterion: a replacement raw `sqp_` value passed format validation inside the paused scanner process without being printed or persisted. Restored the ADR to `In Progress` pending governance validation and the non-secret execution signal. | @codex |
| 2026-08-31 | At `2026-08-31T16:58:18+08:00`, recorded a new authorization blocker from `In Progress`: SonarQube accepted the connection but rejected the replacement token for Execute Analysis on project `koduck`; scanner exit 3 preceded upload and no report or analysis was created. | @codex |
| 2026-08-31 | At `2026-08-31T17:01:37+08:00`, satisfied the authorization blocker exit criterion with a newly generated `koduck` project token supplied only through the system clipboard; its raw format passed inside the paused scanner process without being printed or persisted. Restored the ADR to `In Progress`. | @codex |
| 2026-08-31 | Completed and verified the first `koduck` analysis: exact revision `8a9c5c8b1c2d1c8faa157f80374f51faf2ebe044`, compute task `a0c652a0-e42c-4dd2-b372-9680d54ca327`, scanner exit 0, Quality Gate `Passed`, and authenticated dashboard first-analysis evidence. Removed all exact disposable paths, confirmed credential markers absent from the temporary log, and preserved the original user's two ADR modifications. | @codex |
| 2026-08-31 | @linhai issued `Deprecate` at 2026-08-31T22:01:15+08:00; archived the completed baseline decision with no replacement record. | @codex |
