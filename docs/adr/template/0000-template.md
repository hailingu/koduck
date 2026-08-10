<!-- markdownlint-disable MD041 -->
<!-- Template Instructions

- Before copying or numbering this template, inspect Full ADR and Lightweight
  ADR rows in `docs/adr/INDEX.md`; OCR rows do not block ADR serialization. Stop
  if any ADR has an Implementation Status other than `Complete`, `Verified`, or
  `Not Applicable`, unless every blocker-resolution exception condition in
  `AGENTS.md` is satisfied. Only a Full ADR created from this template may use
  that exception.
- For product demand, select exactly one `Ready` task candidate from one
  `Current` ADD. In the same change, mark that candidate `Selected` with this
  ADR's exact path and record the ADD's exact path plus candidate ID in this
  ADR's `Architecture Source`.
- After routing, numbering, and naming this ADR, add its row to
  `docs/adr/INDEX.md` in the same change that creates the file.
- In Context, explain how the selected ADD candidate becomes this one complete
  ADR task; the ADD supplies solution boundaries and this ADR owns detailed
  design, implementation, and verification.
- List only constraints that bind this decision, and do not duplicate tensions.
- In Open Questions, retain resolved material questions for the decision trail
  and do not request approval while any row remains unresolved. If no material
  question exists, replace the sample row and table with
  `None — no material questions`.
- In Decision Rationale, do not repeat option pros and cons verbatim.
- Define one to three subtasks that together deliver the complete task outcome;
  do not use lifecycle gates as subtasks unless they are actual deliverables.
- Define every acceptance check before approval. Each row verifies one subtask
  and must supply deterministic pass/fail inputs, method, expected result, and
  evidence under the canonical wording rules in `AGENTS.md`.
- Before approval, define objective completion criteria and evidence types for
  every checklist item. Use stable evidence rather than workspace line numbers.
- Put related ADR, issue, and PR links only in Metadata `Related`.
- Replace every variable in retained or triggered content.
- Duplicate a repeatable row or block once per item, then replace its variables.
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
- To replace a non-terminal ADR, first deprecate it to `Not Applicable`; only
  after that terminal state is indexed may the replacement be drafted and
  accepted, followed by the old ADR's transition to `Superseded`.
- An approval-invalidating change resets Decision Status to `Proposed` and
  Implementation Status to `Not Started` in the same change. First preserve the
  old Approver, Approval Time, Approval Evidence, optional Approval Context
  Revision, and invalidation details in Change Log. Then set active Approver,
  Approval Time, and Approval Evidence to `Pending — reapproval required` and
  remove the active Approval Context Revision until a later approval records a
  new applicable value.
- Do not request approval until every acceptance check is binary, deterministic,
  and complete enough to execute without interpreting the meaning of success.
- Remove this section and the Variable Dictionary from the instantiated ADR.
- Finish with no unresolved `{{...}}` placeholders.

Variable Dictionary

### Required Variables

| Variable | Meaning |
|----------|---------|
| `{{ADR_NUMBER}}` | Sequential decimal ADR number in this directory; pad 1 through 9999 to four digits, then continue with 10000 and higher; use it in the title and filename |
| `{{TITLE}}` | Short decision title |
| `{{DATE}}` | Date this ADR was first drafted (YYYY-MM-DD); later revisions belong in Change Log |
| `{{AUTHOR}}` | Drafting agent or person |
| `{{DECISION_OWNER}}` | Person accountable for the decision |
| `{{REQUIRED_APPROVER}}` | Concrete `@<actor-id>` or rule identifying who is authorized to approve |
| `{{RECORD_SCOPE}}` | `Project` or `Service internal — <service>`; source for the central index Scope column |
| `{{CONTEXT_AND_PROBLEM_STATEMENT}}` | The problem, its trigger, and the context needed to understand the decision |
| `{{TASK_OUTCOME}}` | The single end-to-end, objectively verifiable outcome delivered by this ADR |
| `{{SELECTED_OPTION}}` | The option chosen from Options Considered |
| `{{DECISION_RATIONALE}}` | Why the selected option best satisfies the decision drivers and resolves the tensions |
| `{{POSITIVE_CONSEQUENCES}}` | Expected benefits of the decision |
| `{{NEGATIVE_CONSEQUENCES}}` | Expected downsides of the decision |
| `{{MITIGATIONS}}` | How the negative consequences are addressed |
| `{{AFFECTED_PATHS}}` | Modules, services, or files this decision touches |

### Conditional Variables

| Variable | Trigger and meaning |
|----------|---------------------|
| `{{ARCHITECTURE_SOURCE}}` | Product demand — exact ADD path plus task-candidate ID in `<path> — <candidate-id>` form; otherwise `N/A — <reason>` |
| `{{SUPERSEDES}}` | This ADR replaces another — repository-relative replaced ADR path; otherwise the explicit template value `None` |
| `{{MIGRATION_AND_ROLLBACK_STRATEGY}}` | Existing behavior changes — forward migration, stop conditions, and rollback path; otherwise `N/A — <reason>` |

### Optional Variables

| Variable | Meaning |
|----------|---------|
| `{{RELATED_LINKS}}` | Related Trello card, issue, PR, review or approval context, or ADR references |
| `{{APPROVAL_CONTEXT_REVISION}}` | Optional immutable revision that exactly represents the approved document content; informational and non-binding, otherwise remove this field |
| `{{OPTIONAL_SUPPORTING_NOTE}}` | Useful supporting context that is not required for acceptance, implementation, or verification |

### Repeatable Row Variables

| Variable | Row/Block | Meaning |
|----------|-----------|---------|
| `{{TENSION_ID}}` | Identified Tensions | Short identifier, e.g. TN-1 |
| `{{TENSION}}` | Identified Tensions | The competing goals in tension |
| `{{TENSION_IMPACT}}` | Identified Tensions | What happens if left unresolved |
| `{{TENSION_RESOLUTION}}` | Identified Tensions | How this ADR resolves it |
| `{{CONSTRAINT}}` | Constraints | One non-negotiable boundary |
| `{{IN_SCOPE_ITEM}}` | Scope | One behavior, system, module, or deliverable included |
| `{{OUT_OF_SCOPE_ITEM}}` | Scope | One explicitly excluded behavior or change |
| `{{QUESTION_ID}}` | Open Questions | Short identifier, e.g. Q-1 |
| `{{QUESTION}}` | Open Questions | The unresolved question |
| `{{QUESTION_OWNER}}` | Open Questions | Who must answer it |
| `{{QUESTION_DUE}}` | Open Questions | Date it must be answered by |
| `{{QUESTION_STATUS}}` | Open Questions | Current resolution status |
| `{{QUESTION_RESOLUTION_AND_EVIDENCE}}` | Open Questions | The answer and a stable link or reference supporting it |
| `{{DRIVER}}` | Decision Drivers | Name of the driver |
| `{{DRIVER_RATIONALE}}` | Decision Drivers | Why it matters |
| `{{OPTION_NAME}}` | Options Considered | Option name |
| `{{OPTION_DESCRIPTION}}` | Options Considered | What the option does |
| `{{OPTION_PROS}}` | Options Considered | Its advantages |
| `{{OPTION_CONS}}` | Options Considered | Its disadvantages |
| `{{SUBTASK_ID}}` | Implementation Plan | Stable identifier from T-1 through T-3 |
| `{{SUBTASK_OBJECTIVE}}` | Implementation Plan | One objective or deliverable that contributes to the single task outcome |
| `{{SUBTASK_SCOPE}}` | Implementation Plan | Included paths, systems, or boundaries for this subtask |
| `{{CHECK_ID}}` | Acceptance Checks | Stable check identifier, e.g. AC-1 |
| `{{CHECK_SUBTASK_ID}}` | Acceptance Checks | Exactly one declared subtask ID verified by this check |
| `{{ACCEPTANCE_POINT}}` | Acceptance Checks | One binary pass/fail assertion with an unambiguous subject and condition |
| `{{CHECK_PRECONDITIONS}}` | Acceptance Checks | Exact starting state, fixture, environment, or input |
| `{{VERIFICATION_METHOD}}` | Acceptance Checks | Executable command, named automated test, or deterministic inspection procedure |
| `{{EXPECTED_RESULT}}` | Acceptance Checks | Exact observable output, state, invariant, or numeric threshold required for Pass |
| `{{CHECK_EXPECTED_EVIDENCE}}` | Acceptance Checks | Command result, test report, stable artifact, or inspection record to capture |
| `{{CHANGE_DATE}}` | Change Log | Date of the entry |
| `{{CHANGE_DESCRIPTION}}` | Change Log | What changed |
| `{{CHANGE_AUTHOR}}` | Change Log | Who made the change |

-->

---

<!-- Filename: ADR-{{ADR_NUMBER}}-<slug>.md in the routed ADR root: project `docs/adr/` or service `<service>/docs/adr/`; never an `ocr/` directory. -->

# ADR-{{ADR_NUMBER}}: {{TITLE}}

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
- **Related [Optional]**: {{RELATED_LINKS}}
- **Architecture Source [Conditionally Required — product demand]**: {{ARCHITECTURE_SOURCE}}
- **Supersedes [Conditionally Required — this ADR replaces another]**: {{SUPERSEDES}}
- **Superseded By [Conditionally Required — this ADR is replaced]**: None — replace with the exact repository-relative path when triggered

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

{{CONTEXT_AND_PROBLEM_STATEMENT}}

## Scope [Required]

In scope:

- {{IN_SCOPE_ITEM}}

Out of scope:

- {{OUT_OF_SCOPE_ITEM}}

## Tensions, Constraints, And Open Questions [Required]

### Identified Tensions [Conditionally Required — competing goals or trade-offs exist]

| ID | Tension | Impact | Decision |
| --- | --- | --- | --- |
| {{TENSION_ID}} | {{TENSION}} | {{TENSION_IMPACT}} | {{TENSION_RESOLUTION}} |

### Constraints [Required]

- {{CONSTRAINT}}

### Open Questions [Conditionally Required — material questions exist or were resolved during drafting]

| ID | Question | Owner | Due | Status | Resolution and Evidence |
| --- | --- | --- | --- | --- | --- |
| {{QUESTION_ID}} | {{QUESTION}} | {{QUESTION_OWNER}} | {{QUESTION_DUE}} | {{QUESTION_STATUS}} | {{QUESTION_RESOLUTION_AND_EVIDENCE}} |

## Decision Drivers [Required]

1. **{{DRIVER}}**: {{DRIVER_RATIONALE}}

## Options Considered [Required]

### Option: {{OPTION_NAME}}

{{OPTION_DESCRIPTION}}

Pros:

- {{OPTION_PROS}}

Cons:

- {{OPTION_CONS}}

## Decision [Required]

**Selected option**: {{SELECTED_OPTION}}

**Rationale**: {{DECISION_RATIONALE}}

### Consequences [Required]

Positive:

- {{POSITIVE_CONSEQUENCES}}

Negative:

- {{NEGATIVE_CONSEQUENCES}}

Mitigations:

- {{MITIGATIONS}}

## Implementation Plan [Required]

**Complete task outcome**: {{TASK_OUTCOME}}

Allowed subtask statuses: `Not Started`, `In Progress`, `Blocked`, `Complete`,
or `N/A — <specific reason>`.

| ID | Objective or deliverable | Included scope | Status | Actual implementation evidence |
| --- | --- | --- | --- | --- |
| {{SUBTASK_ID}} | {{SUBTASK_OBJECTIVE}} | {{SUBTASK_SCOPE}} | Not Started | Pending |

**Affected paths**: {{AFFECTED_PATHS}}

**Migration and rollback strategy [Conditionally Required — this replaces or
changes existing behavior]**: {{MIGRATION_AND_ROLLBACK_STRATEGY}}

## Acceptance Checks [Required]

| Check ID | Subtask | Binary acceptance point | Preconditions or input | Verification method | Exact expected result | Expected evidence | Status | Actual result and evidence |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| {{CHECK_ID}} | {{CHECK_SUBTASK_ID}} | {{ACCEPTANCE_POINT}} | {{CHECK_PRECONDITIONS}} | {{VERIFICATION_METHOD}} | {{EXPECTED_RESULT}} | {{CHECK_EXPECTED_EVIDENCE}} | Not Started | Pending |

Allowed final check statuses are `Pass`, `Fail`, or `N/A — <specific reason>`.
`Fail` blocks completion. `N/A` is valid only when the check's stated trigger or
precondition demonstrably does not apply.

## Completion Checklist [Required]

| ID | Item | Completion Criterion | Expected Evidence | Status | Actual Evidence |
| --- | --- | --- | --- | --- | --- |
| A-1 | ADR approved | An eligible non-author approver, approval time, and exact `Approval Evidence: Approve` are recorded; any optional Approval Context Revision is informational, non-binding, and exactly represents the approved document | ADR metadata | Not Started | Pending |
| A-2 | Complete task delivered | Every declared subtask has actual implementation evidence, every applicable acceptance check is `Pass` with actual result and evidence, and together they satisfy the complete task outcome | Implementation Plan and Acceptance Checks rows | Not Started | Pending |
| A-3 | Reciprocal ADD link synchronized, when applicable | The selected candidate records this exact ADR path, this ADR records the exact ADD path and candidate ID, both references agree, and the candidate reaches `Complete` only with this ADR's `Complete` or `Verified` status | Exact ADD path, candidate ID, ADR path, and Git blob or commit | Not Started / N/A | Pending |
| A-4 | Requirement levels satisfied | Every required section is complete, every conditional trigger is assessed and completed or marked `N/A — <reason>`, and optional sections are complete or removed | Structured document review | Not Started | Pending |
| A-5 | Acceptance checks are decidable | Every check names one subtask, preconditions or input, deterministic method, exact expected result, and evidence; no unqualified subjective criterion remains | Structured acceptance-check review | Not Started | Pending |

## Supporting Notes [Optional]

{{OPTIONAL_SUPPORTING_NOTE}}

## Archival [Conditionally Required — Decision Status is `Rejected`, or Decision Status is `Deprecated` or `Superseded` and Implementation Status is final]

Archive this record in the same change that retires it when either Decision
Status is `Rejected` with Implementation Status `Not Applicable`, or Decision
Status is `Deprecated` or `Superseded` with Implementation Status `Verified`,
`Complete`, or `Not Applicable`. Before that trigger, retain this section as
inactive future-lifecycle guidance; its checklist does not affect acceptance or
implementation completion. When triggered:

- [ ] Move this file to `archive/ADR-{{ADR_NUMBER}}-<slug>.md` under this same ADR
      root (project `docs/adr/archive/` or `<service>/docs/adr/archive/`).
- [ ] Update every code marker that cites this file's pre-archive path to the new
      archive path, or remove the marker if the governed code was deleted.
- [ ] If Decision Status is `Superseded`, set the replacement record's
      `Supersedes` field and this record's `Superseded By` field to each other's
      final repository-relative path.
- [ ] If no record supersedes this one, retain `Superseded By: None`.
- [ ] Update this record's single row in `docs/adr/INDEX.md` with the archived
      path, scope, and final status.
- [ ] Confirm no ADR or OCR outside an `archive/` directory, and no code marker, still cites the pre-archive path.

## Change Log [Required]

| Date | Change | Author |
| --- | --- | --- |
| {{CHANGE_DATE}} | {{CHANGE_DESCRIPTION}} | {{CHANGE_AUTHOR}} |
