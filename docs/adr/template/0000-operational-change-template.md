<!-- markdownlint-disable MD041 -->
<!-- Template Instructions

- After routing, numbering, and naming this OCR, add its row to
  `docs/adr/INDEX.md` in the same change that creates the file.
- Define one to three subtasks that together deliver the complete operational
  outcome; treat preflight, execution, verification, recovery, and archival as
  lifecycle stages unless one is itself a deliverable.
- Use this template only when every Eligibility item is true; otherwise stop
  and use `0000-template.md` for a Full ADR.
- Before a release or remote Git tag operation, read
  `docs/delivery/releases.md` and `docs/delivery/git-tags.md` in full.
- For Docker preflight, capture the reviewed source, expected image coordinates,
  and immutable digest or local image ID plan. For Kubernetes preflight,
  discover the context, namespace, workload, reviewed configuration revision,
  intended image, and previous recoverable state. Never record secrets.
- Kubernetes verification must cover rollout completion, readiness, running
  image identity, required health checks, and relevant events or diagnostics.
- Stop when a stop condition occurs. Record whether recovery ran, why it was
  skipped when unneeded, and stable target-, time-, exit-, and result-bearing
  evidence for every executed stage.
- Complete Conditional Extensions only for their stated trigger.
- Put Trello and pull-request references in Metadata `Related`; neither changes
  this record's status or supplies approval without the canonical exact
  `Approve` response.
- Replace every variable in retained or triggered content.
- Complete conditional variables when their trigger applies; otherwise use
  `N/A — <reason>`.
- Duplicate the Change Log row once per entry, then replace its variables.
- Keep every `[Required]` section. Complete a `[Conditionally Required]`
  section when its trigger applies; otherwise replace its sample content with
  `N/A — <reason>`. Remove an `[Optional]` field or section when it adds no value.
- Approval requires only the exact response `Approve`; do not require a commit,
  blob, content hash, or revision ID. Record an optional informational approval
  context revision only under the conditions in `AGENTS.md`.
- Rejecting a Proposed record requires the exact response `Reject` from the
  Decision Owner or an actor authorized by Required Approver. Record the
  rejection fields and set Decision Status to `Rejected` and Implementation
  Status to `Not Applicable` in the same change.
- Retirement requires the exact response `Deprecate` or `Supersede` from the
  Decision Owner or an actor authorized by Required Approver. Record every
  retirement field and a truthful final Implementation Status in the same
  change; `Supersede` also requires reciprocal replacement paths.
- An approval-invalidating change resets Decision Status to `Proposed` and
  Implementation Status to `Not Started` in the same change. First preserve the
  old Approver, Approval Time, Approval Evidence, optional Approval Context
  Revision, and invalidation details in Change Log. Then set active Approver,
  Approval Time, and Approval Evidence to `Pending — reapproval required` and
  remove the active Approval Context Revision until a later approval records a
  new applicable value.
- Remove this section and the Variable Dictionary from the instantiated record.
- Finish with no unresolved `{{...}}` placeholders.

Variable Dictionary

### Required Variables

| Variable | Meaning |
|----------|---------|
| `{{OCR_NUMBER}}` | Sequential decimal OCR number in this directory's `ocr/` subfolder; pad 1 through 9999 to four digits, then continue with 10000 and higher; use it in the title and filename |
| `{{TITLE}}` | Short operation title |
| `{{DATE}}` | Date this OCR was first drafted (YYYY-MM-DD); later revisions belong in Change Log |
| `{{AUTHOR}}` | Drafting agent or person |
| `{{DECISION_OWNER}}` | Person accountable for the operation |
| `{{REQUIRED_APPROVER}}` | Concrete `@<actor-id>` or rule identifying who is authorized to approve |
| `{{RECORD_SCOPE}}` | `Project` or `Service internal — <service>`; source for the central index Scope column |
| `{{TARGET_SCOPE}}` | Environment, service, or N/A |
| `{{OPERATION_OWNER}}` | Person executing the operation |
| `{{INPUT_SOURCE_OR_VERSION}}` | Immutable source commit for a build, or immutable input artifact/version for another operation |
| `{{EXPECTED_OUTPUT}}` | Planned artifact coordinates or target state; use N/A only when the operation produces neither |
| `{{DEPENDENCIES}}` | Approved prerequisites this operation relies on |
| `{{TASK_OUTCOME}}` | The single end-to-end, objectively verifiable operational outcome delivered by this OCR |
| `{{PREFLIGHT_ACTION}}` | Artifact, target baseline, approval, and recovery readiness to confirm before executing |
| `{{EXECUTE_ACTION}}` | The safe command or manual operation to run, with secrets omitted; fenced code blocks are allowed |
| `{{VERIFY_CRITERION}}` | The objective success criterion; for build-only operations, include disposition and no-promotion confirmation |
| `{{STOP_CONDITION}}` | The observable condition that stops execution or promotion |
| `{{RECOVERY_ACTION}}` | The rollback action, or artifact quarantine/discard action for a build |
| `{{RECOVERY_VERIFICATION}}` | The objective check that confirms recovery or no promotion |

### Conditional Variables

Required when the stated trigger applies; otherwise use `N/A — <reason>`.

| Variable | Meaning |
|----------|---------|
| `{{DOCKER_IMAGE_IDENTITY}}` | Container operation — expected image coordinates; when consuming an existing image, its immutable repository digest or local image ID plus cluster load path |
| `{{KUBERNETES_TARGET}}` | Kubernetes operation — context, namespace, and workload discovered at preflight |
| `{{ARCHITECTURE_SOURCE}}` | Governing ADR or ADD task applies — exact source path and candidate when applicable |
| `{{SUPERSEDES}}` | This OCR replaces another — repository-relative replaced OCR path; otherwise the explicit template value `None` |
| `{{WINDOW_AND_IMPACT}}` | Start/end/timezone, affected users/services, and expected impact |
| `{{OBSERVATION}}` | Owner, duration, signals and success criteria, and actual result |
| `{{COMMUNICATION}}` | Before/during/after audience, channel, and evidence |

### Optional Variables

| Variable | Meaning |
|----------|---------|
| `{{RELATED_LINKS}}` | Related Trello card, issue, PR, exact review revision, ADR, incident, or change ticket references |
| `{{APPROVAL_CONTEXT_REVISION}}` | Optional immutable revision that exactly represents the approved document content; informational and non-binding, otherwise remove this field |
| `{{OPTIONAL_SUPPORTING_NOTE}}` | Useful operational context that is not required for approval, execution, recovery, or verification |

### Repeatable Row Variables

| Variable | Row | Meaning |
|----------|-----|---------|
| `{{CHANGE_DATE}}` | Change Log | Date of the entry |
| `{{CHANGE_DESCRIPTION}}` | Change Log | What changed |
| `{{CHANGE_AUTHOR}}` | Change Log | Who made the change |
| `{{SUBTASK_ID}}` | Task Definition | Stable identifier from T-1 through T-3 |
| `{{SUBTASK_OBJECTIVE}}` | Task Definition | One operational objective or deliverable that contributes to the single outcome |
| `{{SUBTASK_SCOPE}}` | Task Definition | Included target, artifact, or boundary for this subtask |
| `{{SUBTASK_CRITERION}}` | Task Definition | Objective pass condition for this subtask |
| `{{SUBTASK_EXPECTED_EVIDENCE}}` | Task Definition | Stable evidence type expected for this subtask |

-->

---

<!-- Filename: OCR-{{OCR_NUMBER}}-<slug>.md in the routed OCR root: project `docs/adr/ocr/` or service `<service>/docs/adr/ocr/`; never directly in an ADR root. -->

# OCR-{{OCR_NUMBER}}: {{TITLE}}

## Metadata [Required]

- **Decision Status**: Proposed / Accepted / Rejected / Deprecated / Superseded
- **Implementation Status**: Not Started / In Progress / Blocked / Complete / Verified / Not Applicable
- **Date**: {{DATE}}
- **Author**: {{AUTHOR}}
- **Decision Owner**: {{DECISION_OWNER}}
- **Required Approver**: {{REQUIRED_APPROVER}}
- **Record Scope**: {{RECORD_SCOPE}}
- **Approver [Conditionally Required — Decision Status is or has been `Accepted`]**: Pending — replace with the concrete `@<actor-id>`
- **Approval Time [Conditionally Required — Decision Status is or has been `Accepted`]**: Pending — replace with an ISO 8601 date-time containing `Z` or an explicit `±HH:MM` offset
- **Approval Evidence [Conditionally Required — Decision Status is or has been `Accepted`]**: Pending — replace with exactly `Approve`
- **Approval Context Revision [Optional — informational and non-binding]**: {{APPROVAL_CONTEXT_REVISION}}
- **Rejector [Conditionally Required — Decision Status is `Rejected`]**: Pending — replace with the concrete `@<actor-id>`
- **Rejection Time [Conditionally Required — Decision Status is `Rejected`]**: Pending — replace with an ISO 8601 date-time containing `Z` or an explicit `±HH:MM` offset
- **Rejection Evidence [Conditionally Required — Decision Status is `Rejected`]**: Pending — replace with exactly `Reject`
- **Retired By [Conditionally Required — Decision Status is `Deprecated` or `Superseded`]**: Pending — replace with the concrete `@<actor-id>`
- **Retirement Time [Conditionally Required — Decision Status is `Deprecated` or `Superseded`]**: Pending — replace with an ISO 8601 date-time containing `Z` or an explicit `±HH:MM` offset
- **Retirement Evidence [Conditionally Required — Decision Status is `Deprecated` or `Superseded`]**: Pending — replace with exactly `Deprecate` or `Supersede`
- **Retirement Reason [Conditionally Required — Decision Status is `Deprecated` or `Superseded`]**: Pending
- **Blocked From [Conditionally Required — Implementation Status is `Blocked`]**: Pending — `Not Started` or `In Progress`
- **Blocker And Evidence [Conditionally Required — Implementation Status is `Blocked`]**: Pending
- **Blocker Owner [Conditionally Required — Implementation Status is `Blocked`]**: Pending
- **Blocker Exit Or Recheck Criterion [Conditionally Required — Implementation Status is `Blocked`]**: Pending
- **Operation Type**: Build / Release / Deploy / Rollback / Existing Runbook
- **Target Scope / Operation Owner**: {{TARGET_SCOPE}} / {{OPERATION_OWNER}}
- **Input Source or Version**: {{INPUT_SOURCE_OR_VERSION}}
- **Expected Output or Target State**: {{EXPECTED_OUTPUT}}
- **Docker Image Coordinates or Input Identity [Conditionally Required — container operation]**: {{DOCKER_IMAGE_IDENTITY}}
- **Kubernetes Target [Conditionally Required — Kubernetes operation]**: {{KUBERNETES_TARGET}}
- **Actual immutable artifact [Conditionally Required — operation builds or consumes an artifact]**: Pending
- **Dependencies**: {{DEPENDENCIES}}
- **Related [Optional]**: {{RELATED_LINKS}}
- **Architecture Source [Conditionally Required — a governing ADR or ADD task applies]**: {{ARCHITECTURE_SOURCE}}
- **Supersedes [Conditionally Required — this OCR replaces another]**: {{SUPERSEDES}}
- **Superseded By [Conditionally Required — this OCR is replaced]**: None — replace with the exact repository-relative path when triggered

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

**Complete task outcome**: {{TASK_OUTCOME}}

Allowed subtask statuses: `Not Started`, `In Progress`, `Blocked`, `Complete`,
or `N/A — <specific reason>`.

| ID | Objective or deliverable | Included scope or target | Completion criterion | Expected evidence | Status | Actual evidence |
| --- | --- | --- | --- | --- | --- | --- |
| {{SUBTASK_ID}} | {{SUBTASK_OBJECTIVE}} | {{SUBTASK_SCOPE}} | {{SUBTASK_CRITERION}} | {{SUBTASK_EXPECTED_EVIDENCE}} | Not Started | Pending |

## Eligibility [Required]

- [ ] Uses an accepted architecture, pipeline, artifact contract, security boundary, and data boundary.
- [ ] Is reversible; build-only recovery is artifact quarantine or discard plus no-promotion evidence.
- [ ] Does not modify a Dockerfile, Makefile, CI, pipeline, artifact format,
      signing, credentials, deployment topology, API/schema/protocol,
      authentication, security policy, data lifecycle, dependency, provider,
      or irreversible behavior.
- [ ] Has a defined preflight, success check, stop condition, and recovery/rollback path.
- [ ] Contains no secret, credential, private endpoint, or sensitive user data.
- [ ] When an automatic-review mechanism is configured, it covers the exact
      declared input revision; otherwise this item records `N/A — no
      automatic-review mechanism configured`. Review success is not treated as
      approval or execution evidence.

## Core Runbook And Evidence [Required]

### Preflight [Required]

**Planned action and criterion**: {{PREFLIGHT_ACTION}}

**Actual result and stable evidence**: Pending

### Execute [Required]

**Planned action**: {{EXECUTE_ACTION}}

**Actual result and stable evidence**: Pending

### Verify [Required]

**Success criterion**: {{VERIFY_CRITERION}}

**Actual result and stable evidence**: Pending

### Stop and Recovery [Required]

**Stop condition**: {{STOP_CONDITION}}

**Recovery action**: {{RECOVERY_ACTION}}

**Recovery verification**: {{RECOVERY_VERIFICATION}}

**Actual result and stable evidence**: Pending — if recovery was not triggered,
record "Not triggered" and the observation that confirms it.

## Conditional Extensions [Conditionally Required — production, multi-environment, phased, user/downstream/SLO impact, or stated change-window operation]

- **Window and impact**: {{WINDOW_AND_IMPACT}}
- **Observation**: {{OBSERVATION}}
- **Communication**: {{COMMUNICATION}}

## Closure [Required]

Allowed review statuses for Authorization review, Subtask and evidence review,
and Requirement-level review are `Pass`, `Fail`, or `N/A — <specific reason>`.
`Fail` blocks `Complete` and `Verified`; stop and recover, roll back, or use a
truthful retirement path as applicable. `N/A` is valid only when the review's
stated condition does not apply.

- **Final result**: Pending — completed / stopped / rolled back / not promoted
- **Authorization review**: Pending — replace with `Pass` only when Decision
  Status was `Accepted`, complete approval metadata was recorded, and Approval
  Time preceded the first Execute action; use `Fail` if execution began without
  satisfying those conditions, or `N/A — operation not executed` with rejection
  or retirement evidence when no Execute action occurred
- **Subtask and evidence review**: Pending — replace with `Pass` only when every
  declared subtask and applicable core or extension item has actual stable
  evidence and together they satisfy the complete task outcome
- **Requirement-level review**: Pending — replace with `Pass` only when required
  content for the target status is complete, every conditional trigger is
  completed or marked `N/A — <reason>`, and optional content is complete or
  removed

## Supporting Notes [Optional]

{{OPTIONAL_SUPPORTING_NOTE}}

## Archival [Conditionally Required — Decision Status is retired or Implementation Status is final]

Once Decision Status is `Deprecated` or `Superseded`, or Implementation Status
reaches a final state (`Verified`, `Complete`, `Blocked` with no further attempt
planned, or `Not Applicable`), archive this record in the same change that
establishes that archival-eligible state.

Before that trigger, retain this section as inactive future-lifecycle guidance;
its checklist does not affect approval or operation completion. When triggered:

- [ ] Move this file to `ocr/archive/OCR-{{OCR_NUMBER}}-<slug>.md` under this same
      ADR root (project `docs/adr/ocr/archive/` or `<service>/docs/adr/ocr/archive/`).
- [ ] Update every code marker that cites this file's pre-archive path to the new
      archive path, or remove the marker if the governed artifact/config was reverted.
- [ ] If Decision Status is `Superseded`, set the replacement record's
      `Supersedes` field and this record's `Superseded By` field to each other's
      final repository-relative path.
- [ ] If no record supersedes this one, retain `Superseded By: None`.
- [ ] Update this record's single row in `docs/adr/INDEX.md` with the archived
      path, scope, and final status.
- [ ] Confirm no ADR or OCR outside an `archive/` directory, and no governed
      marker, still cites the pre-archive path.

## Change Log [Required]

| Date | Change | Author |
| --- | --- | --- |
| {{CHANGE_DATE}} | {{CHANGE_DESCRIPTION}} | {{CHANGE_AUTHOR}} |
