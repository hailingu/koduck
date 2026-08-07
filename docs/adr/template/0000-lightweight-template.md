<!-- markdownlint-disable MD041 -->
<!-- Template Instructions

- Replace every variable listed in the Variable Dictionary below.
- Duplicate a repeatable row once per item, then replace its variables.
- Remove this section and the Variable Dictionary from the instantiated ADR.
- Finish with no unresolved `{{...}}` placeholders.

Variable Dictionary

### Required Variables

| Variable | Meaning |
|----------|---------|
| `{{ADR_NUMBER}}` | Four-digit sequential ADR number in this directory, used both in the title (`Lightweight ADR-{{ADR_NUMBER}}`) and the filename (`ADR-{{ADR_NUMBER}}-<slug>.md`) |
| `{{TITLE}}` | Short decision title |
| `{{DATE}}` | Date this revision was drafted or last changed (YYYY-MM-DD) |
| `{{AUTHOR}}` | Drafting agent or person |
| `{{DECISION_OWNER}}` | Person accountable for the decision |
| `{{REQUIRED_APPROVER}}` | Concrete `@<actor-id>` or rule identifying who is authorized to approve |
| `{{RELATED_LINKS}}` | Related Trello card, issue, PR, revision-bound review or approval comment, or ADR references |
| `{{SUPERSEDES}}` | Repository-relative path of the record this ADR replaces, or `None` |
| `{{CONTEXT}}` | The small, low-risk change needed and what triggered it |
| `{{DECISION}}` | What will change |

### Repeatable Row Variables

| Variable | Row/Block | Meaning |
|----------|-----------|---------|
| `{{IN_SCOPE_ITEM}}` | Scope | One file, component, or module included |
| `{{OUT_OF_SCOPE_ITEM}}` | Scope | One explicitly excluded behavior or change |
| `{{IMPLEMENTATION_STEP}}` | Implementation Plan | One implementation step |
| `{{EVIDENCE_ITEM}}` | Completion Evidence | The item being verified |
| `{{COMPLETION_CRITERION}}` | Completion Evidence | The observable result required |
| `{{EVIDENCE}}` | Completion Evidence | Commit SHA / Git blob / PR permalink / symbol / heading / command result |
| `{{NOTE}}` | Notes | An optional clarifying note |
| `{{CHANGE_DATE}}` | Change Log | Date of the entry |
| `{{CHANGE_DESCRIPTION}}` | Change Log | What changed |
| `{{CHANGE_AUTHOR}}` | Change Log | Who made the change |

-->

---

<!-- Filename: ADR-{{ADR_NUMBER}}-<slug>.md, stored in this directory's `docs/adr/` root (not `docs/adr/ocr/`). -->

# Lightweight ADR-{{ADR_NUMBER}}: {{TITLE}}

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

## Context

{{CONTEXT}}

## Decision

{{DECISION}}

## Scope

In scope:

- {{IN_SCOPE_ITEM}}

Out of scope:

- {{OUT_OF_SCOPE_ITEM}}

## Lightweight Eligibility Check

- [ ] Change is small, localized, and reversible.
- [ ] No public API, schema, protocol, service boundary, auth, security, data model, migration, dependency, build, CI, or deployment behavior changes.
- [ ] No new technology, framework, storage, queue, runtime, provider, or design pattern.
- [ ] No unresolved product or technical question changes the chosen approach.
- [ ] Verification evidence can be captured with a focused test, static check, screenshot, or code link.

If any item is false, use `0000-template.md` instead.

## Implementation Plan

- [ ] {{IMPLEMENTATION_STEP}}

## Completion Evidence

| Item | Completion criterion | Evidence |
| ------ | -------------------- | ------- |
| {{EVIDENCE_ITEM}} | {{COMPLETION_CRITERION}} | {{EVIDENCE}} |

Evidence must identify the verified content and result. Workspace line numbers
may be included only as navigation aids because they drift after later edits.
Do not mark Implementation Status `Verified` until every criterion has stable
evidence.

Trello is coordination context only. A card move, closure, label, reaction, or
ordinary comment does not change this record. If Trello or pull-request approval
evidence is used, it must identify the concrete approver, this record's
repository-relative path, and the exact approved Git blob or commit.

## Notes

- {{NOTE}}

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
| ------ | ------ | ------ |
| {{CHANGE_DATE}} | {{CHANGE_DESCRIPTION}} | {{CHANGE_AUTHOR}} |
