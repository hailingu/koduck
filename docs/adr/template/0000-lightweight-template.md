<!-- markdownlint-disable MD041 -->
<!-- Template Instructions

- Before copying or numbering this template, inspect Full ADR and Lightweight
  ADR rows in `docs/adr/INDEX.md`; OCR rows do not block ADR serialization. Stop
  if any ADR has an Implementation Status other than `Complete`, `Verified`, or
  `Not Applicable`. A blocker-resolution exception must use the Full ADR
  template and cannot use this template.
- For product demand, select exactly one `Ready` task candidate from one
  `Current` ADD. In the same change, mark that candidate `Selected` with this
  ADR's exact path and record the ADD's exact path plus candidate ID in this
  ADR's `Architecture Source`.
- After routing, numbering, and naming this ADR, add its row to
  `docs/adr/INDEX.md` in the same change that creates the file.
- In Context, explain how a selected ADD candidate, when applicable, becomes
  this one complete ADR task; the ADD supplies solution context and boundaries,
  while this ADR owns detailed design, implementation, and verification.
- Keep the ADR to one independently reviewable implementation slice deliverable
  through one implementation pull request and name its one primary
  implementation boundary. Split the ADD candidate before selection when that
  is not possible.
- Use this template only when every Lightweight Eligibility Check item is true;
  otherwise stop and use `0000-template.md` for a Full ADR.
- Define one to three subtasks that together deliver the complete task outcome;
  do not use lifecycle gates as subtasks unless they are actual deliverables.
- When implementation exceeds or waives the common software-engineering
  standard, complete the Engineering Exceptions subsection with one row per
  affected unit before approval. A pull request may link to the exception but
  cannot authorize it. Replace the sample table with `N/A — <reason>` when no
  engineering exception applies.
- Define every acceptance check before approval. Each row verifies one subtask
  and must supply deterministic pass/fail inputs, method, expected result, and
  evidence under the canonical wording rules in `AGENTS.md`.
- Complete Contract-To-Check Traceability and duplicate the Risk Coverage
  Matrix sample row exactly once for each of the five baseline dimensions
  required by `AGENTS.md`. Every applicable risk row must link to one or more
  acceptance checks; use only a specific `N/A` reason for a dimension that
  cannot apply.
- For product demand, keep both ADD/ADR references synchronized in one change
  and mark the candidate `Complete` only when this ADR is `Complete` or
  `Verified`.
- Put Trello and pull-request references in Metadata `Related`; neither changes
  this record's status or supplies approval without the canonical exact
  `Approve` response.
- Replace every variable in retained or triggered content.
- Duplicate a repeatable row once per item, then replace its variables.
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
| `{{CONTEXT}}` | The small, low-risk change needed and what triggered it |
| `{{DECISION}}` | What will change |
| `{{TASK_OUTCOME}}` | The single independently reviewable, objectively verifiable outcome delivered by this ADR |
| `{{PRIMARY_IMPLEMENTATION_BOUNDARY}}` | The one primary implementation boundary for this source change |

### Conditional Variables

| Variable | Trigger and meaning |
|----------|---------------------|
| `{{ARCHITECTURE_SOURCE}}` | Product demand — exact ADD path plus task-candidate ID in `<path> — <candidate-id>` form; otherwise `N/A — <reason>` |
| `{{SUPERSEDES}}` | This ADR replaces another — repository-relative replaced ADR path; otherwise the explicit template value `None` |

### Optional Variables

| Variable | Meaning |
|----------|---------|
| `{{RELATED_LINKS}}` | Related Trello card, issue, PR, review or approval context, or ADR references |
| `{{APPROVAL_CONTEXT_REVISION}}` | Optional immutable revision that exactly represents the approved document content; informational and non-binding, otherwise remove this field |
| `{{NOTE}}` | Useful supporting context that is not required for acceptance, implementation, or verification |

### Repeatable Row Variables

| Variable | Row/Block | Meaning |
|----------|-----------|---------|
| `{{IN_SCOPE_ITEM}}` | Scope | One file, component, or module included |
| `{{OUT_OF_SCOPE_ITEM}}` | Scope | One explicitly excluded behavior or change |
| `{{SUBTASK_ID}}` | Implementation Plan | Stable identifier from T-1 through T-3 |
| `{{SUBTASK_OBJECTIVE}}` | Implementation Plan | One objective or deliverable that contributes to the single task outcome |
| `{{SUBTASK_SCOPE}}` | Implementation Plan | Included paths, components, or boundaries for this subtask |
| `{{EXCEPTION_RULE_AND_UNIT}}` | Engineering Exceptions | Exact exceeded or waived rule plus affected repository-relative path and symbol |
| `{{EXCEPTION_MEASURED_VALUE}}` | Engineering Exceptions | Measured value or structural condition that triggers the exception |
| `{{EXCEPTION_RATIONALE}}` | Engineering Exceptions | Why the unit remains cohesive or why compliance is currently unsafe |
| `{{EXCEPTION_RISKS}}` | Engineering Exceptions | Risks created by retaining the exceptional condition |
| `{{EXCEPTION_CONTROLS}}` | Engineering Exceptions | Compensating controls, including focused tests or ownership restrictions |
| `{{EXCEPTION_OWNER}}` | Engineering Exceptions | One accountable `@<actor-id>` |
| `{{EXCEPTION_REVIEW_OR_REMOVAL}}` | Engineering Exceptions | Removal or review date, or specific permanent rationale |
| `{{EXCEPTION_VERIFICATION}}` | Engineering Exceptions | Evidence that demonstrates the compensating controls |
| `{{CONTRACT_CLAUSE_ID}}` | Contract-To-Check Traceability | Stable identifier for one normative contract clause |
| `{{CONTRACT_LOCATION}}` | Contract-To-Check Traceability | Exact repository-relative contract path plus heading or anchor |
| `{{CONTRACT_REQUIREMENT}}` | Contract-To-Check Traceability | Exact required response, transition, ordering, invariant, limit, failure outcome, or prohibition |
| `{{CONTRACT_CHECK_IDS}}` | Contract-To-Check Traceability | Acceptance-check or deterministic-test IDs that explicitly exercise this clause |
| `{{CONTRACT_COVERAGE_METHOD}}` | Contract-To-Check Traceability | How the linked checks exercise the complete clause |
| `{{RISK_DIMENSION}}` | Risk Coverage Matrix | One required baseline dimension from `AGENTS.md`; duplicate the row exactly five times |
| `{{RISK_SCENARIO_OR_NA}}` | Risk Coverage Matrix | Concrete failure or edge scenario, or `N/A — <specific reason>` |
| `{{RISK_BOUNDARY}}` | Risk Coverage Matrix | Component, adapter, framework, datastore, or trust boundary that owns the behavior |
| `{{RISK_VERIFICATION_METHOD}}` | Risk Coverage Matrix | Deterministic test or inspection that exercises the scenario |
| `{{RISK_EXPECTED_RESULT}}` | Risk Coverage Matrix | Exact observable result required for Pass |
| `{{RISK_CHECK_IDS}}` | Risk Coverage Matrix | Linked acceptance-check IDs |
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

# Lightweight ADR-{{ADR_NUMBER}}: {{TITLE}}

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

## Context [Required]

{{CONTEXT}}

## Decision [Required]

{{DECISION}}

## Scope [Required]

In scope:

- {{IN_SCOPE_ITEM}}

Out of scope:

- {{OUT_OF_SCOPE_ITEM}}

## Lightweight Eligibility Check [Required]

- [ ] Change is limited to the paths in Scope and can be fully reverted without migration or retained partial behavior.
- [ ] Change modifies only localized source behavior and its tests; it does not modify normative governance documentation or configuration.
- [ ] No public API, schema, protocol, service boundary, auth, security, data model, migration, dependency, build, CI, or deployment behavior changes.
- [ ] No new technology, framework, storage, queue, runtime, provider, or design pattern.
- [ ] No material product or technical question remains unresolved.
- [ ] Verification can use a deterministic focused test, static check, or inspection that produces an exact observable result and stable evidence.

## Implementation Plan [Required]

**Complete task outcome**: {{TASK_OUTCOME}}

**Primary implementation boundary**: {{PRIMARY_IMPLEMENTATION_BOUNDARY}}

Allowed subtask statuses: `Not Started`, `In Progress`, `Blocked`, `Complete`,
or `N/A — <specific reason>`.

| ID | Objective or deliverable | Included scope | Status | Actual implementation evidence |
| --- | --- | --- | --- | --- |
| {{SUBTASK_ID}} | {{SUBTASK_OBJECTIVE}} | {{SUBTASK_SCOPE}} | Not Started | Pending |

### Engineering Exceptions [Conditionally Required — an engineering rule is exceeded or waived]

| Rule and affected unit | Measured value or condition | Rationale | Risks | Compensating controls | Owner | Removal, review, or permanent rationale | Verification evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| {{EXCEPTION_RULE_AND_UNIT}} | {{EXCEPTION_MEASURED_VALUE}} | {{EXCEPTION_RATIONALE}} | {{EXCEPTION_RISKS}} | {{EXCEPTION_CONTROLS}} | {{EXCEPTION_OWNER}} | {{EXCEPTION_REVIEW_OR_REMOVAL}} | {{EXCEPTION_VERIFICATION}} |

## Contract-To-Check Traceability [Required]

| Clause ID | Authoritative contract path and heading | Exact normative requirement | Acceptance check or deterministic test IDs | Explicit coverage method |
| --- | --- | --- | --- | --- |
| {{CONTRACT_CLAUSE_ID}} | {{CONTRACT_LOCATION}} | {{CONTRACT_REQUIREMENT}} | {{CONTRACT_CHECK_IDS}} | {{CONTRACT_COVERAGE_METHOD}} |

## Risk Coverage Matrix [Required]

| Risk dimension | Applicability and scenario, or specific N/A reason | Owning boundary | Deterministic verification method | Exact expected result | Acceptance check IDs | Status | Actual evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| {{RISK_DIMENSION}} | {{RISK_SCENARIO_OR_NA}} | {{RISK_BOUNDARY}} | {{RISK_VERIFICATION_METHOD}} | {{RISK_EXPECTED_RESULT}} | {{RISK_CHECK_IDS}} | Not Started | Not run — implementation not started |

Duplicate the sample row exactly once for each baseline dimension: concurrency
and ordering; timeout and deadline; cancellation and interruption; resource
bounds and backpressure; and framework or trust-boundary rejection. Allowed
final statuses are `Pass`, `Fail`, or `N/A — <specific reason>`. `Fail` blocks
review-ready, `Complete`, and `Verified`.

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
| A-6 | Engineering exceptions governed, when applicable | Every exceeded or waived engineering rule has one complete exception row, an accountable owner, a lifecycle, and verification evidence before approval; otherwise the conditional subsection records `N/A — <reason>` | Engineering Exceptions subsection and affected-file evidence | Not Started | Pending |
| A-7 | Contract and baseline risks covered | Every normative contract clause maps to an explicit check or deterministic test, and every required Risk Coverage Matrix row is complete before approval and reaches Pass or specific N/A before review-ready or completion | Contract-To-Check Traceability, Risk Coverage Matrix, acceptance checks, and stable evidence | Not Started | Pending |

## Notes [Optional]

- {{NOTE}}

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
