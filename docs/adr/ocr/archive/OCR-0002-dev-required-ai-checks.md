<!-- markdownlint-disable MD041 -->

---

# OCR-0002: Dev Required Koduck AI Checks

## Metadata [Required]

- **Decision Status**: Accepted
- **Implementation Status**: Verified
- **Date**: 2026-08-12
- **Author**: @codex
- **Decision Owner**: @linhai
- **Required Approver**: @linhai
- **Record Scope**: Project
- **Approver [Conditionally Required — Decision Status is or has been `Accepted`]**: @linhai
- **Approval Time [Conditionally Required — Decision Status is or has been `Accepted`]**: 2026-08-12T14:24:58+08:00
- **Approval Evidence [Conditionally Required — Decision Status is or has been `Accepted`]**: Approve
- **Rejector [Conditionally Required — Decision Status is `Rejected`]**: N/A — Decision Status is `Accepted`
- **Rejection Time [Conditionally Required — Decision Status is `Rejected`]**: N/A — Decision Status is `Accepted`
- **Rejection Evidence [Conditionally Required — Decision Status is `Rejected`]**: N/A — Decision Status is `Accepted`
- **Retired By [Conditionally Required — Decision Status is `Deprecated` or `Superseded`]**: N/A — Decision Status is `Accepted`
- **Retirement Time [Conditionally Required — Decision Status is `Deprecated` or `Superseded`]**: N/A — Decision Status is `Accepted`
- **Retirement Evidence [Conditionally Required — Decision Status is `Deprecated` or `Superseded`]**: N/A — Decision Status is `Accepted`
- **Retirement Reason [Conditionally Required — Decision Status is `Deprecated` or `Superseded`]**: N/A — Decision Status is `Accepted`
- **Blocked From [Conditionally Required — Implementation Status is `Blocked`]**: N/A — Implementation Status is `Verified`
- **Blocker And Evidence [Conditionally Required — Implementation Status is `Blocked`]**: N/A — Implementation Status is `Verified`
- **Blocker Owner [Conditionally Required — Implementation Status is `Blocked`]**: N/A — Implementation Status is `Verified`
- **Blocker Exit Or Recheck Criterion [Conditionally Required — Implementation Status is `Blocked`]**: N/A — Implementation Status is `Verified`
- **Operation Type**: Existing Runbook
- **Target Scope / Operation Owner**: GitHub repository `hailingu/koduck`, branch `dev` / @codex
- **Input Source or Version**: Accepted `docs/adr/ADR-0002-required-ai-ci-postgres-verification.md`; PR 1 exact commit `f1154c59e73b52c2309485dfe45ab952517cd0c5`
- **Expected Output or Target State**: `dev` branch protection requires exactly `koduck-ai-format`, `koduck-ai-clippy`, and `koduck-ai-test-postgres`, each bound to GitHub Actions App ID `15368`, with strict up-to-date status checks; no review-count, administrator-enforcement, push-restriction, or ruleset policy is added
- **Docker Image Coordinates or Input Identity [Conditionally Required — container operation]**: N/A — no container operation
- **Kubernetes Target [Conditionally Required — Kubernetes operation]**: N/A — no Kubernetes operation
- **Actual immutable artifact [Conditionally Required — operation builds or consumes an artifact]**: N/A — repository settings operation produces no artifact
- **Dependencies**: Accepted `docs/adr/ADR-0002-required-ai-ci-postgres-verification.md`; public repository visibility; repository Admin permission; three successful GitHub Actions checks on the exact input commit
- **Related [Optional]**: [Pull request 1](https://github.com/hailingu/koduck/pull/1), [final automatic review](https://github.com/hailingu/koduck/pull/1#issuecomment-5263030272)
- **Architecture Source [Conditionally Required — a governing ADR or ADD task applies]**: `docs/adr/ADR-0002-required-ai-ci-postgres-verification.md`, subtask T-3
- **Supersedes [Conditionally Required — this OCR replaces another]**: None
- **Superseded By [Conditionally Required — this OCR is replaced]**: None

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

Planned content is required before `Accepted`; actual-result fields remain
`Pending` until their corresponding operation stage runs and are required
before `Complete` or `Verified`.

## Task Definition [Required]

**Complete task outcome**: Protect `hailingu/koduck` branch `dev` with strict
required status checks bound to GitHub Actions App ID `15368` for exactly
`koduck-ai-format`, `koduck-ai-clippy`, and `koduck-ai-test-postgres`; verify
the API state and PR 1 merge gate, or restore the captured unprotected baseline
if any execution or verification condition fails.

Allowed subtask statuses: `Not Started`, `In Progress`, `Blocked`, `Complete`,
or `N/A — <specific reason>`.

| ID | Objective or deliverable | Included scope or target | Completion criterion | Expected evidence | Status | Actual evidence |
| --- | --- | --- | --- | --- | --- | --- |
| T-1 | Require the three approved revision-bound checks on `dev`. | GitHub branch protection for `hailingu/koduck:dev`; required status checks only. | Protection API reports `strict: true` and exactly the three named checks with App ID `15368`; administrator enforcement is disabled, review requirements and push restrictions are absent, and repository rulesets remain empty. | Before/after branch-protection and ruleset JSON, API exit status, and PR 1 required-check/merge-state evidence. | Complete | At `2026-08-12T14:27:35+08:00`, protection reported `strict: true` and exactly the three approved contexts with App ID `15368`; administrator enforcement was false, reviews/restrictions were null, rulesets remained `[]`, and PR 1 was `CLEAN` / `MERGEABLE` with all three checks successful and zero current unresolved threads. |

## Eligibility [Required]

- [x] Uses the Accepted ADR-0002 CI pipeline, check-name contract, security
      boundary, and data boundary.
- [x] Is reversible: the captured baseline is no branch protection and no
      repository rulesets; rollback deletes only the newly created `dev`
      branch protection and verifies restoration of that baseline.
- [x] Does not modify a Dockerfile, Makefile, CI, pipeline, artifact format,
      signing, credentials, deployment topology, API/schema/protocol,
      authentication, security policy, data lifecycle, dependency, provider,
      or irreversible behavior; it applies the existing Accepted ADR-0002
      required-check contract.
- [x] Has a defined preflight, success check, stop condition, and rollback path.
- [x] Contains no secret, credential, private endpoint, or sensitive user data.
- [x] The configured automatic review covers exact input commit
      `f1154c59e73b52c2309485dfe45ab952517cd0c5` and reported no major issues in
      PR comment `5263030272`; this evidence is not treated as OCR approval or
      execution evidence.

## Core Runbook And Evidence [Required]

### Preflight [Required]

**Planned action and criterion**:

1. Confirm this OCR is `Accepted`, records `@linhai` as approver with exact
   `Approval Evidence: Approve`, and its approval time precedes Execute.
2. Confirm `hailingu/koduck` remains public, the authenticated actor has Admin
   permission, `dev` exists, and its current commit is recorded without
   changing or synchronizing the branch.
3. Confirm `GET /repos/hailingu/koduck/branches/dev/protection` returns exactly
   `404 Branch not protected` and `GET /repos/hailingu/koduck/rulesets` returns
   `[]`; any other result means the captured rollback baseline is stale.
4. Confirm exact input commit
   `f1154c59e73b52c2309485dfe45ab952517cd0c5` has successful check runs named
   `koduck-ai-format`, `koduck-ai-clippy`, and `koduck-ai-test-postgres`, each
   owned by GitHub Actions App ID `15368`, and final automatic review comment
   `5263030272` covers that commit without an actionable finding.
5. Confirm the operation changes no repository visibility, workflow, source,
   branch content, ruleset, review-count requirement, administrator
   enforcement, push restriction, tag, release, or deployment.

**Actual result and stable evidence**: Pass. Before Execute, repository
visibility was `public`, the authenticated actor had Admin permission, `dev`
pointed to `799868b9b67204ec1c618c1af124bee3d07291d3`, protection returned `404
Branch not protected`, and rulesets returned `[]`. Exact input commit
`f1154c59e73b52c2309485dfe45ab952517cd0c5` had all three approved successful
checks from GitHub Actions App ID `15368`; final automatic review comment
`5263030272` covered that commit and reported no major issue. One read-only
branch request encountered a transient TLS certificate error; its retry
succeeded before Execute and confirmed the unchanged baseline.

### Execute [Required]

**Planned action**: Send one `PUT` request to
`/repos/hailingu/koduck/branches/dev/protection` with this exact non-secret
payload:

```json
{
  "required_status_checks": {
    "strict": true,
    "checks": [
      {"context": "koduck-ai-format", "app_id": 15368},
      {"context": "koduck-ai-clippy", "app_id": 15368},
      {"context": "koduck-ai-test-postgres", "app_id": 15368}
    ]
  },
  "enforce_admins": false,
  "required_pull_request_reviews": null,
  "restrictions": null
}
```

Do not retry a non-success response automatically. Do not modify repository
rulesets or any additional protection option.

**Actual result and stable evidence**: Pass between the completed preflight at
`2026-08-12T14:25:56+08:00` and completed verification at
`2026-08-12T14:27:35+08:00`. The single approved `PUT` succeeded and returned
`strict: true`, the three approved context/App-ID pairs, `enforce_admins:
false`, `required_pull_request_reviews: null`, and `restrictions: null`. No
retry or additional settings mutation ran.

### Verify [Required]

**Success criterion**: All of the following are true:

1. `GET /repos/hailingu/koduck/branches/dev/protection` succeeds and reports
   strict required status checks containing exactly the three approved context
   names, each with App ID `15368`.
2. The same response reports administrator enforcement disabled, no required
   pull-request-review policy, and no push restrictions; the repository
   rulesets API still returns `[]`.
3. PR 1 still targets `dev`; its exact head commit has all three checks with
   conclusion `success`, its merge state has no required-check blocker, and no
   current non-outdated actionable Review thread exists.
4. No repository visibility, workflow, source, branch content, tag, release,
   artifact, deployment, or unrelated setting changed.

**Actual result and stable evidence**: Pass at
`2026-08-12T14:27:35+08:00`. The protection API returned exactly the approved
strict checks and no additional policy; rulesets remained `[]`; `dev` remained
at `799868b9b67204ec1c618c1af124bee3d07291d3` and reported `protected: true`.
PR 1 still targeted `dev`, head
`f1154c59e73b52c2309485dfe45ab952517cd0c5` was `CLEAN` / `MERGEABLE`, and
`koduck-ai-format`, `koduck-ai-clippy`, and `koduck-ai-test-postgres` all
reported `SUCCESS`. The GraphQL thread check returned zero current,
non-outdated unresolved threads. A transient TLS error interrupted the first
combined read after protection and rulesets had already verified; the retry
completed the remaining read-only checks without observing configuration
drift.

### Stop and Recovery [Required]

**Stop condition**: Stop before Execute if approval is absent or stale, the
repository is not public, Admin permission is absent, `dev` does not exist, the
captured no-protection/no-ruleset baseline has changed, any exact check is
missing, unsuccessful, or owned by a different App ID, the automatic review
does not cover the input commit, or the requested payload differs from the
approved payload. After Execute, stop and recover if the API response is not
successful or any Verify criterion fails.

**Recovery action**: If Execute created or partially created protection before
a stop condition, send `DELETE
/repos/hailingu/koduck/branches/dev/protection`. Do not delete or modify any
ruleset because the captured baseline contains none. Do not change repository
visibility, branch content, workflows, tags, releases, or unrelated settings.

**Recovery verification**: Confirm the protection endpoint again returns
exactly `404 Branch not protected`, the rulesets endpoint returns `[]`, `dev`
still points to its recorded preflight commit, and no source, workflow, tag,
release, artifact, deployment, or unrelated setting changed.

**Actual result and stable evidence**: Not triggered. The approved `PUT` and
every verification criterion succeeded. The no-protection baseline was not
restored because `dev` now exposes exactly the intended required checks; no
ruleset, branch content, workflow, visibility, tag, release, artifact,
deployment, or unrelated setting changed.

## Conditional Extensions [Conditionally Required — production, multi-environment, phased, user/downstream/SLO impact, or stated change-window operation]

- **Window and impact**: Execute immediately after approval in this task; no
  runtime downtime or user-data impact. After success, pushes and merges into
  `dev` must satisfy the three approved checks.
- **Observation**: @codex observes the protection response, ruleset baseline,
  PR 1 exact-revision check conclusions, merge state, and current Review thread
  count immediately after Execute.
- **Communication**: Record authorization and final evidence in this OCR and
  PR 1, then report the result in the active task; no production incident or
  customer communication is triggered.

## Closure [Required]

Allowed review statuses for Authorization review, Subtask and evidence review,
and Requirement-level review are `Pass`, `Fail`, or `N/A — <specific reason>`.
`Fail` blocks `Complete` and `Verified`; stop and recover, roll back, or use a
truthful retirement path as applicable. `N/A` is valid only when the review's
stated condition does not apply.

- **Final result**: Completed. `dev` requires exactly the three approved strict
  GitHub Actions checks; rollback was not triggered.
- **Authorization review**: Pass — OCR Decision Status was `Accepted`,
  `@linhai` supplied exact `Approve`, and the recorded approval time
  `2026-08-12T14:24:58+08:00` preceded Execute.
- **Subtask and evidence review**: Pass — T-1 is `Complete`; the captured
  baseline, exact execution response, protection/ruleset verification, PR 1
  status, and no-rollback evidence satisfy the complete task outcome.
- **Requirement-level review**: Pass — all required and triggered content for
  `Verified` is complete, and every non-triggered conditional has a specific
  N/A reason.
- **Governance validation**: Pass — governance validation passed (`npm run validate --prefix tools/governance-validator`).

## Archival [Conditionally Required — Decision Status is retired or Implementation Status is final]

Once Decision Status is `Deprecated` or `Superseded`, or Implementation Status
reaches a final state (`Verified`, `Complete`, `Blocked` with no further attempt
planned, or `Not Applicable`), archive this record in the same change that
establishes that archival-eligible state.

Before that trigger, retain this section as inactive future-lifecycle guidance;
its checklist does not affect approval or operation completion. When triggered:

- [x] Move this file to `ocr/archive/OCR-0002-dev-required-ai-checks.md` under
      `docs/adr/`.
- [x] Update every code marker that cites this file's pre-archive path to the
      new archive path, or remove it if the governed artifact/config was
      reverted.
- [x] N/A — Decision Status is `Accepted`, not `Superseded`; reciprocal
      supersession metadata is not triggered.
- [x] Retain `Superseded By: None` when no record supersedes this one.
- [x] Update this record's single row in `docs/adr/INDEX.md` with its archived
      path and final status.
- [x] Confirm no active record or governed marker cites the pre-archive path.

## Change Log [Required]

| Date | Change | Author |
| --- | --- | --- |
| 2026-08-12 | Executed the single approved branch-protection request, verified the exact strict GitHub Actions checks, unchanged ruleset/branch baselines, PR 1 `CLEAN` / `MERGEABLE` state, three successful checks, and zero current unresolved threads. Marked the operation `Verified`; recovery was not triggered; archived the terminal OCR and updated all references. | @codex |
| 2026-08-12 | Recorded `@linhai`'s exact `Approve` at `2026-08-12T14:24:58+08:00`, set the OCR to `Accepted / In Progress`, and began the approved preflight. | @codex |
| 2026-08-12 | Proposed one reversible GitHub repository-settings operation after public visibility removed ADR-0002's plan-level blocker. Captured `dev` as unprotected, repository rulesets as empty, the three exact successful GitHub Actions checks at input commit `f1154c59e73b52c2309485dfe45ab952517cd0c5`, and an exact delete-protection rollback. No external setting was mutated. | @codex |
