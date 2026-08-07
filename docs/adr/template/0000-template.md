<!-- markdownlint-disable MD041 -->
<!-- Template Instructions

- Replace every variable listed in the Variable Dictionary below.
- Duplicate a repeatable row or block once per item, then replace its variables.
- Remove this section and the Variable Dictionary from the instantiated ADR.
- Finish with no unresolved `{{...}}` placeholders.

Variable Dictionary

### Required Variables

| Variable | Meaning |
|----------|---------|
| `{{ADR_NUMBER}}` | Four-digit sequential ADR number in this directory, used both in the title (`ADR-{{ADR_NUMBER}}`) and the filename (`ADR-{{ADR_NUMBER}}-<slug>.md`) |
| `{{TITLE}}` | Short decision title |
| `{{DATE}}` | Date this revision was drafted or last changed (YYYY-MM-DD) |
| `{{AUTHOR}}` | Drafting agent or person |
| `{{DECISION_OWNER}}` | Person accountable for the decision |
| `{{REQUIRED_APPROVER}}` | Concrete `@<actor-id>` or rule identifying who is authorized to approve |
| `{{RELATED_LINKS}}` | Related Trello card, issue, PR, revision-bound review or approval comment, or ADR references |
| `{{SUPERSEDES}}` | Repository-relative path of the record this ADR replaces, or `None` |
| `{{CONTEXT_AND_PROBLEM_STATEMENT}}` | The problem, its trigger, and the context needed to understand the decision |
| `{{SELECTED_OPTION}}` | The option chosen from Options Considered |
| `{{DECISION_RATIONALE}}` | Why the selected option best satisfies the decision drivers and resolves the tensions |
| `{{POSITIVE_CONSEQUENCES}}` | Expected benefits of the decision |
| `{{NEGATIVE_CONSEQUENCES}}` | Expected downsides of the decision |
| `{{MITIGATIONS}}` | How the negative consequences are addressed |
| `{{AFFECTED_PATHS}}` | Modules, services, or files this decision touches |
| `{{MIGRATION_AND_ROLLBACK_STRATEGY}}` | Forward migration approach, stop conditions, and rollback path, if applicable |

### Repeatable Row Variables

| Variable | Row/Block | Meaning |
|----------|-----------|---------|
| `{{TENSION_ID}}` | Identified Tensions | Short identifier, e.g. C-1 |
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
| `{{IMPLEMENTATION_STEP}}` | Implementation Plan | One implementation step |
| `{{CHECKLIST_ID}}` | Completion Checklist | Short identifier, e.g. A-2 |
| `{{CHECKLIST_ITEM}}` | Completion Checklist | What must be true |
| `{{COMPLETION_CRITERION}}` | Completion Checklist | Objective pass condition |
| `{{EXPECTED_EVIDENCE}}` | Completion Checklist | Evidence type expected |
| `{{CHANGE_DATE}}` | Change Log | Date of the entry |
| `{{CHANGE_DESCRIPTION}}` | Change Log | What changed |
| `{{CHANGE_AUTHOR}}` | Change Log | Who made the change |

-->

---

<!-- Filename: ADR-{{ADR_NUMBER}}-<slug>.md, stored in this directory's `docs/adr/` root (not `docs/adr/ocr/`). -->

# ADR-{{ADR_NUMBER}}: {{TITLE}}

## Metadata

- **Decision Status**: Proposed / Accepted / Rejected / Deprecated / Superseded
- **Implementation Status**: Not Started / In Progress / Blocked / Complete / Verified / Not Applicable
- **Date**: {{DATE}}
- **Author**: {{AUTHOR}}
- **Decision Owner**: {{DECISION_OWNER}}
- **Required Approver**: {{REQUIRED_APPROVER}}
- **Approver**: Pending — replace with the concrete `@<actor-id>`
- **Approval Time**: Pending
- **Approval Evidence**: Pending — revision-bound task, Trello, or review permalink that identifies the approver, this path, and the approved Git revision
- **Approval Version**: Pending — Git blob ID of this ADR before approval
- **Related**: {{RELATED_LINKS}}
- **Supersedes**: {{SUPERSEDES}}
- **Superseded By**: Pending — repository-relative path once another record replaces this one, otherwise `None`

## Context And Problem Statement

{{CONTEXT_AND_PROBLEM_STATEMENT}}

## Scope

In scope:

- {{IN_SCOPE_ITEM}}

Out of scope:

- {{OUT_OF_SCOPE_ITEM}}

## Tensions, Constraints, And Open Questions

### Identified Tensions

| ID | Tension | Impact | Decision |
| --- | --- | --- | --- |
| {{TENSION_ID}} | {{TENSION}} | {{TENSION_IMPACT}} | {{TENSION_RESOLUTION}} |

### Constraints

Non-negotiable boundaries this decision must respect (security, compatibility,
data, cost, timeline, or scope limits). Only list constraints that actually bind
this decision — do not restate the tensions table.

- {{CONSTRAINT}}

### Open Questions

Record questions whose answers could materially affect the decision, and retain
their rows after resolution to preserve the decision trail. Decision Status must
not be `Accepted` while any question in this table is unresolved. If there are no
such questions, replace the sample row and table with "None."

| ID | Question | Owner | Due | Status | Resolution and Evidence |
| --- | --- | --- | --- | --- | --- |
| {{QUESTION_ID}} | {{QUESTION}} | {{QUESTION_OWNER}} | {{QUESTION_DUE}} | {{QUESTION_STATUS}} | {{QUESTION_RESOLUTION_AND_EVIDENCE}} |

## Decision Drivers

1. **{{DRIVER}}**: {{DRIVER_RATIONALE}}

## Options Considered

### Option: {{OPTION_NAME}}

{{OPTION_DESCRIPTION}}

Pros:

- {{OPTION_PROS}}

Cons:

- {{OPTION_CONS}}

(Duplicate this block once per option considered.)

## Decision

**Selected option**: {{SELECTED_OPTION}}

**Rationale**: {{DECISION_RATIONALE}} (do not repeat the option's pros/cons verbatim)

### Consequences

Positive:

- {{POSITIVE_CONSEQUENCES}}

Negative:

- {{NEGATIVE_CONSEQUENCES}}

Mitigations:

- {{MITIGATIONS}}

## Implementation Plan

- [ ] {{IMPLEMENTATION_STEP}}

**Affected paths**: {{AFFECTED_PATHS}}

**Migration and rollback strategy** (if this replaces or changes existing
behavior): {{MIGRATION_AND_ROLLBACK_STRATEGY}}

## Completion Checklist

Each item must have an objective, verifiable completion criterion and evidence
type before approval. Evidence must identify stable content — a commit SHA, Git
blob, PR permalink, stable symbol/heading, or a command and its result. Workspace
line numbers are navigation aids only, not evidence. Do not mark Implementation
Status `Verified` until every applicable item has actual evidence recorded.

| ID | Item | Completion Criterion | Expected Evidence | Status | Actual Evidence |
| --- | --- | --- | --- | --- | --- |
| A-1 | ADR approved | Approver, time, evidence, and pre-approval Git blob ID recorded | ADR metadata | Not Started | Pending |
| {{CHECKLIST_ID}} | {{CHECKLIST_ITEM}} | {{COMPLETION_CRITERION}} | {{EXPECTED_EVIDENCE}} | Not Started | Pending |

Link related ADRs, issues, and PRs in the Metadata `Related` field above; do not
maintain a second list here.

Trello is coordination context only. A card move, closure, label, reaction, or
ordinary comment does not change this record. If Trello or pull-request approval
evidence is used, it must identify the concrete approver, this record's
repository-relative path, and the exact approved Git blob or commit.

## Archival

Once Decision Status is `Deprecated` or `Superseded` and Implementation Status is
`Verified`, `Complete`, or `Not Applicable`, archive this record in the same change
that retires it:

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
- [ ] Confirm no other active ADR, OCR, or code marker still cites the pre-archive path.

## Change Log

| Date | Change | Author |
| --- | --- | --- |
| {{CHANGE_DATE}} | {{CHANGE_DESCRIPTION}} | {{CHANGE_AUTHOR}} |
