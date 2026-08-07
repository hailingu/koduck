<!-- markdownlint-disable MD041 -->
<!-- Template Instructions

- Replace every variable listed in the Variable Dictionary below.
- Complete the Conditional Extensions variables only when that section applies.
- Delete the entire Conditional Extensions section when it does not apply.
- Duplicate the Change Log row once per entry, then replace its variables.
- Remove this section and the Variable Dictionary from the instantiated record.
- Finish with no unresolved `{{...}}` placeholders.

Variable Dictionary

### Required Variables

| Variable | Meaning |
|----------|---------|
| `{{OCR_NUMBER}}` | Four-digit sequential OCR number in this directory's `ocr/` subfolder, used both in the title (`OCR-{{OCR_NUMBER}}`) and the filename (`OCR-{{OCR_NUMBER}}-<slug>.md`) |
| `{{TITLE}}` | Short operation title |
| `{{DATE}}` | Date this revision was drafted or last changed (YYYY-MM-DD) |
| `{{AUTHOR}}` | Drafting agent or person |
| `{{DECISION_OWNER}}` | Person accountable for the operation |
| `{{REQUIRED_APPROVER}}` | Concrete `@<actor-id>` or rule identifying who is authorized to approve |
| `{{TARGET_SCOPE}}` | Environment, service, or N/A |
| `{{OPERATION_OWNER}}` | Person executing the operation |
| `{{INPUT_SOURCE_OR_VERSION}}` | Immutable source commit for a build, or immutable input artifact/version for another operation |
| `{{EXPECTED_OUTPUT}}` | Planned artifact coordinates or target state; use N/A only when the operation produces neither |
| `{{DOCKER_IMAGE_IDENTITY}}` | Expected immutable repository digest, or local image ID plus cluster load path; use N/A with reason for non-container operations |
| `{{KUBERNETES_TARGET}}` | Kubernetes context, namespace, and workload discovered at preflight; use N/A with reason for non-Kubernetes operations |
| `{{DEPENDENCIES}}` | Approved prerequisites this operation relies on |
| `{{RELATED_LINKS}}` | Related Trello card, issue, PR, exact review revision, ADR, incident, or change ticket references |
| `{{SUPERSEDES}}` | Repository-relative path of the OCR this operation replaces or corrects, or `None` |
| `{{PREFLIGHT_ACTION}}` | Artifact, target baseline, approval, and recovery readiness to confirm before executing |
| `{{EXECUTE_ACTION}}` | The safe command or manual operation to run, with secrets omitted; fenced code blocks are allowed |
| `{{VERIFY_CRITERION}}` | The objective success criterion; for build-only operations, include disposition and no-promotion confirmation |
| `{{STOP_CONDITION}}` | The observable condition that stops execution or promotion |
| `{{RECOVERY_ACTION}}` | The rollback action, or artifact quarantine/discard action for a build |
| `{{RECOVERY_VERIFICATION}}` | The objective check that confirms recovery or no promotion |

### Conditional Variables

Required only when the Conditional Extensions section applies.

| Variable | Meaning |
|----------|---------|
| `{{WINDOW_AND_IMPACT}}` | Start/end/timezone, affected users/services, and expected impact |
| `{{OBSERVATION}}` | Owner, duration, signals and success criteria, and actual result |
| `{{COMMUNICATION}}` | Before/during/after audience, channel, and evidence |

### Repeatable Row Variables

| Variable | Row | Meaning |
|----------|-----|---------|
| `{{CHANGE_DATE}}` | Change Log | Date of the entry |
| `{{CHANGE_DESCRIPTION}}` | Change Log | What changed |
| `{{CHANGE_AUTHOR}}` | Change Log | Who made the change |

-->

---

<!-- Filename: OCR-{{OCR_NUMBER}}-<slug>.md, stored in this ADR root's `docs/adr/ocr/` subfolder (not `docs/adr/`). -->

# OCR-{{OCR_NUMBER}}: {{TITLE}}

## Metadata

- **Decision Status**: Proposed / Accepted / Rejected / Deprecated / Superseded
- **Implementation Status**: Not Started / In Progress / Blocked / Complete / Verified / Not Applicable
- **Date**: {{DATE}}
- **Author / Decision Owner**: {{AUTHOR}} / {{DECISION_OWNER}}
- **Required Approver**: {{REQUIRED_APPROVER}}
- **Approver**: Pending — replace with the concrete `@<actor-id>`
- **Approval Time**: Pending
- **Approval Evidence**: Pending — revision-bound task, Trello, or review permalink that identifies the approver, this path, and the approved Git revision
- **Approval Version**: Pending — pre-approval Git blob ID
- **Operation Type**: Build / Release / Deploy / Rollback / Existing Runbook
- **Target Scope / Operation Owner**: {{TARGET_SCOPE}} / {{OPERATION_OWNER}}
- **Input Source or Version**: {{INPUT_SOURCE_OR_VERSION}}
- **Expected Output or Target State**: {{EXPECTED_OUTPUT}}
- **Docker Image Identity**: {{DOCKER_IMAGE_IDENTITY}}
- **Kubernetes Target**: {{KUBERNETES_TARGET}}
- **Actual immutable artifact**: Pending — required after a build; otherwise record the immutable artifact used or N/A with reason
- **Dependencies**: {{DEPENDENCIES}}
- **Related**: {{RELATED_LINKS}}
- **Supersedes**: {{SUPERSEDES}}
- **Superseded By**: Pending — repository-relative path once another record replaces this one, otherwise `None`

## Eligibility

- [ ] Uses an accepted architecture, pipeline, artifact contract, security boundary, and data boundary.
- [ ] Is reversible; build-only recovery is artifact quarantine or discard plus no-promotion evidence.
- [ ] Does not change pipeline, contract, signing, credentials, topology, API/schema/protocol, auth,
      security, data lifecycle, dependency, provider, or irreversible behavior.
- [ ] Has a defined preflight, success check, stop condition, and recovery/rollback path.
- [ ] Contains no secret, credential, private endpoint, or sensitive user data.
- [ ] Automatic review covers the exact approved input revision; review success
      is not treated as approval or execution evidence.

If any item is false, stop and use `0000-template.md` for a Full ADR.

## Core Runbook And Evidence

### Preflight

**Planned action and criterion**: {{PREFLIGHT_ACTION}}

**Actual result and stable evidence**: Pending

For a Docker build, confirm the reviewed source revision, expected image
coordinates, and how an immutable digest or local image ID will be captured.
For Kubernetes, discover and record the actual context, namespace, workload,
reviewed manifest or configuration revision, intended image identity, and
previous recoverable state. Never record kubeconfig content or secrets.

### Execute

**Planned action**: {{EXECUTE_ACTION}}

**Actual result and stable evidence**: Pending

### Verify

**Success criterion**: {{VERIFY_CRITERION}}

**Actual result and stable evidence**: Pending

For Kubernetes, verification includes rollout completion, workload readiness,
the running image identity, required health checks, and relevant events or
diagnostics. A successful Git review or card transition is not runtime evidence.

### Stop and Recovery

**Stop condition**: {{STOP_CONDITION}}

**Recovery action**: {{RECOVERY_ACTION}}

**Recovery verification**: {{RECOVERY_VERIFICATION}}

**Actual result and stable evidence**: Pending — if recovery was not triggered,
record "Not triggered" and the observation that confirms it.

Do not continue after a stop condition. Record a skipped or unneeded recovery with its reason.
Prefer a commit SHA, Git blob, immutable log, timestamped dashboard snapshot, permalink, or
command-result summary that includes the target, timestamp, exit status, and relevant output.

Trello is coordination context only. A card move, closure, label, reaction, or
ordinary comment does not accept, execute, verify, or close this OCR. If Trello
or pull-request approval evidence is used, it must identify the concrete
approver, this record's repository-relative path, and the exact approved Git
blob or commit.

## Conditional Extensions

Complete this section only for production, multi-environment or phased operations, expected
user/downstream/SLO impact, or work crossing a stated change window:

- **Window and impact**: {{WINDOW_AND_IMPACT}}
- **Observation**: {{OBSERVATION}}
- **Communication**: {{COMMUNICATION}}

## Closure

- **Final result**: Pending — completed / stopped / rolled back / not promoted
- **Implementation Status**: Update to `Verified` only after every applicable core and extension item has evidence.

## Archival

Once Decision Status is `Deprecated` or `Superseded`, or Implementation Status
reaches a final state (`Verified`, `Complete`, `Blocked` with no further attempt
planned, or `Not Applicable`), archive this record in the same change that retires it:

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

## Change Log

| Date | Change | Author |
| ----------------- | ---------------------- | ------------------ |
| {{CHANGE_DATE}} | {{CHANGE_DESCRIPTION}} | {{CHANGE_AUTHOR}} |
