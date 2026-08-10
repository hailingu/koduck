<!-- markdownlint-disable MD041 -->
<!-- Template Instructions

- Read `AGENTS.md`, `docs/README.md`, and `docs/architecture/INDEX.md` before
  copying this template.
- Route the ADD before assigning its number: repository-wide or cross-boundary
  documents use `docs/architecture/`; single-service or single-package internal
  documents use `<service-or-package>/docs/architecture/`.
- After routing, numbering, and naming this ADD, add its row to
  `docs/architecture/INDEX.md` in the same change that creates the file.
- Capture the Trello baseline used for this revision and report later drift;
  Trello edits do not silently revise the ADD.
- Describe capabilities as externally meaningful behavior, keep data design
  conceptual or logical, and keep architecture and flows at solution level.
  Leave physical schema, source-file plans, commands, and other task-level
  implementation mechanics to selected ADRs.
- Add a small context, container, flowchart, or sequence diagram only when it
  materially clarifies relationships or flow.
- In Assumptions And Open Questions, retain resolved material questions and do
  not request approval while any material question remains unresolved.
- Define each ADR task candidate as one complete outcome with boundaries and no
  implementation plan.
- Trace every requirement to at least one capability and task candidate, plus
  every applicable data, component, and flow reference.
- Replace every variable in retained or triggered content.
- Duplicate repeatable rows once per item, then replace their variables.
- Keep every `[Required]` section. Complete a `[Conditionally Required]`
  section when its trigger applies; otherwise replace its sample content with
  `N/A — <reason>`. Remove an `[Optional]` field or section when it adds no value.
- When an ADR selects a candidate, update the candidate status and exact ADR
  path in the same change that writes the ADR's reciprocal `Architecture
  Source` reference.
- Do not relink a `Selected` candidate directly to another ADR. First reject a
  Proposed linked ADR or retire an Accepted linked ADR to `Not Applicable` and
  return or defer the candidate; completed candidates remain historical and
  replacement work uses a new candidate.
- Approval requires only the exact response `Approve`; do not require a commit,
  blob, content hash, or revision ID. Record an optional informational approval
  context revision only under the conditions in `AGENTS.md`.
- Retirement requires the exact response `Deprecate` or `Supersede` from the
  Architecture Owner or an actor authorized by Required Approver. Record every
  retirement field in the same change; `Supersede` also requires reciprocal
  replacement paths.
- Before retirement, every candidate must be `Deferred` or `Complete`, and no
  ADR that names this ADD in `Architecture Source` may have a non-terminal
  Implementation Status.
- An approval-invalidating change resets Design Status to `Draft` in the same
  change. First preserve the old Approver, Approval Time, Approval Evidence,
  optional Approval Context Revision, and invalidation details in Change Log.
  Then set active Approver, Approval Time, and Approval Evidence to `Pending —
  reapproval required` and remove the active Approval Context Revision until a
  later approval records a new applicable value.
- Remove this section and the Variable Dictionary from the instantiated ADD.
- Finish with no unresolved `{{...}}` placeholders.

Variable Dictionary

### Required Variables

| Variable | Meaning |
| --- | --- |
| `{{ADD_NUMBER}}` | Increasing decimal number unique within this architecture root; pad 1 through 9999 to four digits, then continue with 10000 and higher |
| `{{TITLE}}` | Short solution title |
| `{{DATE}}` | Date this ADD was first drafted (YYYY-MM-DD); later revisions belong in Change Log |
| `{{AUTHOR}}` | Drafting person or agent |
| `{{ARCHITECTURE_OWNER}}` | Person accountable for the overall solution |
| `{{REQUIRED_APPROVER}}` | Concrete `@<actor-id>` or rule identifying who is authorized to approve |
| `{{SCOPE_LEVEL}}` | `Repository / Cross-project` or `Service / Package internal` |
| `{{SCOPE}}` | Projects, services, packages, or capabilities covered |
| `{{TRELLO_SOURCES}}` | Stable URLs of all source Trello cards |
| `{{CONTEXT}}` | Requirement context and the problem the solution must address |
| `{{SOLUTION_SUMMARY}}` | Concise end-to-end solution view |
| `{{DESIGN_BOUNDARY}}` | Explicit boundary between solution design and later ADR implementation design |

### Conditional Variables

| Variable | Trigger and meaning |
| --- | --- |
| `{{FIGMA_SOURCES}}` | UI is in scope — exact Figma file/node URLs; otherwise `N/A — <reason>` |
| `{{SUPERSEDES}}` | This ADD replaces another — repository-relative replaced ADD path; otherwise the explicit template value `None` |

### Optional Variables

| Variable | Meaning |
| --- | --- |
| `{{RELATED_LINKS}}` | Related ADRs, contracts, security documents, issues, or PRs |
| `{{APPROVAL_CONTEXT_REVISION}}` | Optional immutable revision that exactly represents the approved document content; informational and non-binding, otherwise remove this field |
| `{{OPTIONAL_SUPPORTING_MATERIAL}}` | Useful diagrams, alternatives, estimates, references, or explanatory notes not required for review |

### Repeatable Row Variables

| Variable | Section | Meaning |
| --- | --- | --- |
| `{{REQ_ID}}` | Requirement Baseline | Stable local requirement ID, e.g. R-1 |
| `{{TRELLO_URL}}` | Requirement Baseline | Stable source card URL |
| `{{REQUIREMENT}}` | Requirement Baseline | Captured requirement statement |
| `{{ACCEPTANCE_OUTCOME}}` | Requirement Baseline | Observable product outcome requested |
| `{{REQUIREMENT_CONSTRAINTS}}` | Requirement Baseline | Priority and binding constraints |
| `{{LAST_CHECKED}}` | Requirement Baseline | Date the card baseline was last checked |
| `{{GOAL}}` | Goals | One desired solution outcome |
| `{{NON_GOAL}}` | Non-Goals | One explicit exclusion |
| `{{CAPABILITY_ID}}` | Functional Capability Design | Stable ID, e.g. F-1 |
| `{{ACTOR}}` | Functional Capability Design | User, system, or external actor |
| `{{CAPABILITY_TRIGGER}}` | Functional Capability Design | Event that initiates the capability |
| `{{CAPABILITY}}` | Functional Capability Design | Required behavior and outcome |
| `{{BUSINESS_RULES}}` | Functional Capability Design | Rules and important edge cases |
| `{{CAPABILITY_REQUIREMENTS}}` | Functional Capability Design | Requirement IDs satisfied |
| `{{ENTITY_ID}}` | Data Model Design | Stable ID, e.g. D-1 |
| `{{ENTITY}}` | Data Model Design | Conceptual or logical entity |
| `{{ENTITY_PURPOSE}}` | Data Model Design | Why the entity exists |
| `{{OWNERSHIP}}` | Data Model Design | Owning service or domain |
| `{{DATA_CLASSIFICATION}}` | Data Model Design | Sensitivity and retention category |
| `{{ENTITY_LIFECYCLE}}` | Data Model Design | Creation, change, retention, and deletion states |
| `{{RELATIONSHIP}}` | Data Model Design | Entity relationship and cardinality |
| `{{CARDINALITY_MEANING}}` | Data Model Design | Cardinality and semantic meaning of the relationship |
| `{{INVARIANT}}` | Data Model Design | Integrity rule that must remain true |
| `{{COMPONENT_ID}}` | Architecture Design | Stable ID, e.g. C-1 |
| `{{COMPONENT}}` | Architecture Design | System, service, package, datastore, or external dependency |
| `{{RESPONSIBILITY}}` | Architecture Design | High-level responsibility |
| `{{INPUTS_OUTPUTS}}` | Architecture Design | Conceptual inputs, outputs, or events |
| `{{DEPENDENCIES}}` | Architecture Design | Upstream and downstream dependencies |
| `{{ARCHITECTURE_CONSTRAINTS}}` | Architecture Design | Binding accepted decisions or constraints |
| `{{CONTROL_FLOW_ID}}` | Control Flow Design | Stable ID, e.g. CF-1 |
| `{{CONTROL_TRIGGER}}` | Control Flow Design | Event that initiates this control flow |
| `{{CONTROL_PRECONDITION}}` | Control Flow Design | Required starting state |
| `{{CONTROL_HAPPY_PATH}}` | Control Flow Design | Ordered high-level control path |
| `{{CONTROL_BRANCHES}}` | Control Flow Design | Decisions, retries, or alternate paths |
| `{{CONTROL_FAILURE}}` | Control Flow Design | Failure handling and terminal state |
| `{{CONTROL_RESULT}}` | Control Flow Design | Observable result |
| `{{INTERACTION_ID}}` | Interaction Flow Design | Stable ID, e.g. IX-1 |
| `{{ENTRY_STATE}}` | Interaction Flow Design | Actor and entry state |
| `{{USER_ACTIONS}}` | Interaction Flow Design | High-level actions or gestures |
| `{{SYSTEM_FEEDBACK}}` | Interaction Flow Design | Feedback, state transition, and recovery |
| `{{EXIT_STATE}}` | Interaction Flow Design | Success, cancellation, or failure state |
| `{{FIGMA_REFERENCE}}` | Interaction Flow Design | Exact Figma node URL or N/A reason |
| `{{QUALITY_ATTRIBUTE}}` | Cross-Cutting Design | Security, privacy, reliability, observability, performance, scalability, accessibility, or compatibility concern |
| `{{QUALITY_DESIGN}}` | Cross-Cutting Design | Solution-level treatment |
| `{{QUALITY_VALIDATION}}` | Cross-Cutting Design | Observable architecture-level validation |
| `{{ASSUMPTION_ID}}` | Assumptions And Open Questions | Stable ID |
| `{{ASSUMPTION_OR_QUESTION}}` | Assumptions And Open Questions | Assumption or unresolved question |
| `{{QUESTION_OWNER}}` | Assumptions And Open Questions | Person who must resolve it |
| `{{QUESTION_STATUS}}` | Assumptions And Open Questions | Open or Resolved |
| `{{QUESTION_RESOLUTION}}` | Assumptions And Open Questions | Resolution and stable evidence |
| `{{RISK_ID}}` | Risks And Trade-Offs | Stable ID |
| `{{RISK}}` | Risks And Trade-Offs | Risk or trade-off |
| `{{RISK_IMPACT}}` | Risks And Trade-Offs | Consequence if realized |
| `{{MITIGATION}}` | Risks And Trade-Offs | Solution-level mitigation |
| `{{CANDIDATE_ID}}` | ADR Task Candidates | Stable ID with an ADD-candidate-specific prefix, e.g. CAND-1 |
| `{{CANDIDATE_OUTCOME}}` | ADR Task Candidates | One complete, verifiable task outcome |
| `{{CANDIDATE_BOUNDARY}}` | ADR Task Candidates | Included and excluded scope |
| `{{CANDIDATE_DEPENDENCIES}}` | ADR Task Candidates | Prerequisites and ordering constraints |
| `{{CANDIDATE_ACCEPTANCE}}` | ADR Task Candidates | Acceptance context inherited from requirements |
| `{{RECOMMENDED_ADR_TYPE}}` | ADR Task Candidates | Full or Lightweight |
| `{{CANDIDATE_STATUS}}` | ADR Task Candidates | Ready, Selected, Complete, or Deferred |
| `{{CANDIDATE_STATUS_REASON}}` | ADR Task Candidates | Required reason or transition evidence for Deferred, Selected, or Complete; use `N/A — Ready` for Ready |
| `{{ADR_PATH}}` | ADR Task Candidates | Exact repository-relative selected ADR path, or `None` |
| `{{TRACE_REQUIREMENT}}` | Traceability | Requirement ID |
| `{{TRACE_CAPABILITIES}}` | Traceability | Capability IDs |
| `{{TRACE_DATA}}` | Traceability | Entity IDs |
| `{{TRACE_COMPONENTS}}` | Traceability | Component IDs |
| `{{TRACE_FLOWS}}` | Traceability | Control and interaction flow IDs |
| `{{TRACE_CANDIDATES}}` | Traceability | ADR task candidate IDs |
| `{{CHANGE_DATE}}` | Change Log | Date of change |
| `{{CHANGE_DESCRIPTION}}` | Change Log | What changed |
| `{{CHANGE_AUTHOR}}` | Change Log | Who made the change |

-->

---

<!-- Filename: ADD-{{ADD_NUMBER}}-<slug>.md in the routed architecture root: repository/cross-project `docs/architecture/` or service/package `<service-or-package>/docs/architecture/`. -->

# ADD-{{ADD_NUMBER}}: {{TITLE}}

## Metadata [Required]

- **Design Status**: Draft / Current / Deprecated / Superseded
- **Date**: {{DATE}}
- **Author**: {{AUTHOR}}
- **Architecture Owner**: {{ARCHITECTURE_OWNER}}
- **Required Approver**: {{REQUIRED_APPROVER}}
- **Approver [Conditionally Required — Design Status is or has been `Current`]**: Pending — replace with the concrete `@<actor-id>`
- **Approval Time [Conditionally Required — Design Status is or has been `Current`]**: Pending — replace with an ISO 8601 date-time containing `Z` or an explicit `±HH:MM` offset
- **Approval Evidence [Conditionally Required — Design Status is or has been `Current`]**: Pending — replace with exactly `Approve`
- **Approval Context Revision [Optional — informational and non-binding]**: {{APPROVAL_CONTEXT_REVISION}}
- **Retired By [Conditionally Required — Design Status is `Deprecated` or `Superseded`]**: Pending — replace with the concrete `@<actor-id>`
- **Retirement Time [Conditionally Required — Design Status is `Deprecated` or `Superseded`]**: Pending — replace with an ISO 8601 date-time containing `Z` or an explicit `±HH:MM` offset
- **Retirement Evidence [Conditionally Required — Design Status is `Deprecated` or `Superseded`]**: Pending — replace with exactly `Deprecate` or `Supersede`
- **Retirement Reason [Conditionally Required — Design Status is `Deprecated` or `Superseded`]**: Pending
- **Scope Level**: {{SCOPE_LEVEL}}
- **Scope**: {{SCOPE}}
- **Trello Sources**: {{TRELLO_SOURCES}}
- **Figma Sources [Conditionally Required — UI is in scope]**: {{FIGMA_SOURCES}}
- **Related [Optional]**: {{RELATED_LINKS}}
- **Supersedes [Conditionally Required — this ADD replaces another]**: {{SUPERSEDES}}
- **Superseded By [Conditionally Required — this ADD is replaced]**: None — replace with the exact replacement ADD path when triggered

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

## Context And Solution Summary [Required]

{{CONTEXT}}

**Solution summary**: {{SOLUTION_SUMMARY}}

**Design boundary**: {{DESIGN_BOUNDARY}}

## Requirement Baseline [Required]

| ID | Trello source | Requirement baseline | Acceptance outcome | Priority and constraints | Last checked |
| --- | --- | --- | --- | --- | --- |
| {{REQ_ID}} | {{TRELLO_URL}} | {{REQUIREMENT}} | {{ACCEPTANCE_OUTCOME}} | {{REQUIREMENT_CONSTRAINTS}} | {{LAST_CHECKED}} |

## Goals And Non-Goals [Required]

Goals:

- {{GOAL}}

Non-goals:

- {{NON_GOAL}}

## Functional Capability Design [Required]

| ID | Actor | Trigger | Capability and outcome | Business rules and edge cases | Requirements |
| --- | --- | --- | --- | --- | --- |
| {{CAPABILITY_ID}} | {{ACTOR}} | {{CAPABILITY_TRIGGER}} | {{CAPABILITY}} | {{BUSINESS_RULES}} | {{CAPABILITY_REQUIREMENTS}} |

## Data Model Design [Conditionally Required — data is created, updated, deleted, transferred, retained, or changes ownership, classification, lifecycle, relationships, or invariants]

### Entities And Lifecycle

| ID | Entity | Purpose | Ownership | Classification | Lifecycle |
| --- | --- | --- | --- | --- | --- |
| {{ENTITY_ID}} | {{ENTITY}} | {{ENTITY_PURPOSE}} | {{OWNERSHIP}} | {{DATA_CLASSIFICATION}} | {{ENTITY_LIFECYCLE}} |

### Relationships And Invariants

| Relationship | Cardinality and meaning | Invariant |
| --- | --- | --- |
| {{RELATIONSHIP}} | {{CARDINALITY_MEANING}} | {{INVARIANT}} |

## Architecture Design [Required]

| ID | Component or dependency | Responsibility | Conceptual inputs and outputs | Dependencies | Accepted constraints |
| --- | --- | --- | --- | --- | --- |
| {{COMPONENT_ID}} | {{COMPONENT}} | {{RESPONSIBILITY}} | {{INPUTS_OUTPUTS}} | {{DEPENDENCIES}} | {{ARCHITECTURE_CONSTRAINTS}} |

## Control Flow Design [Conditionally Required — the solution has multiple steps, branches, retries, asynchronous work, or failure recovery]

| ID | Trigger and precondition | Happy path | Branches and retries | Failure handling | Observable result |
| --- | --- | --- | --- | --- | --- |
| {{CONTROL_FLOW_ID}} | {{CONTROL_TRIGGER}} / {{CONTROL_PRECONDITION}} | {{CONTROL_HAPPY_PATH}} | {{CONTROL_BRANCHES}} | {{CONTROL_FAILURE}} | {{CONTROL_RESULT}} |

## Interaction Flow Design [Conditionally Required — a human or external system interacts with the solution]

| ID | Actor and entry state | Actions | System feedback and transitions | Exit state | Figma reference |
| --- | --- | --- | --- | --- | --- |
| {{INTERACTION_ID}} | {{ENTRY_STATE}} | {{USER_ACTIONS}} | {{SYSTEM_FEEDBACK}} | {{EXIT_STATE}} | {{FIGMA_REFERENCE}} |

## Cross-Cutting Design [Required]

| Quality attribute | Solution-level design | Architecture-level validation |
| --- | --- | --- |
| {{QUALITY_ATTRIBUTE}} | {{QUALITY_DESIGN}} | {{QUALITY_VALIDATION}} |

## Assumptions And Open Questions [Conditionally Required — assumptions or material questions exist]

| ID | Assumption or question | Owner | Status | Resolution and evidence |
| --- | --- | --- | --- | --- |
| {{ASSUMPTION_ID}} | {{ASSUMPTION_OR_QUESTION}} | {{QUESTION_OWNER}} | {{QUESTION_STATUS}} | {{QUESTION_RESOLUTION}} |

## Risks And Trade-Offs [Required]

| ID | Risk or trade-off | Impact | Mitigation |
| --- | --- | --- | --- |
| {{RISK_ID}} | {{RISK}} | {{RISK_IMPACT}} | {{MITIGATION}} |

## ADR Task Candidates [Required]

Allowed task-candidate statuses: `Ready`, `Selected`, `Complete`, or `Deferred`.

| ID | Complete outcome | Scope boundary | Dependencies | Acceptance context | Recommended ADR type | Status | Status reason or evidence | ADR path |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| {{CANDIDATE_ID}} | {{CANDIDATE_OUTCOME}} | {{CANDIDATE_BOUNDARY}} | {{CANDIDATE_DEPENDENCIES}} | {{CANDIDATE_ACCEPTANCE}} | {{RECOMMENDED_ADR_TYPE}} | {{CANDIDATE_STATUS}} | {{CANDIDATE_STATUS_REASON}} | {{ADR_PATH}} |

## Traceability [Required]

| Requirement | Capabilities | Data entities | Components | Control / interaction flows | ADR task candidates |
| --- | --- | --- | --- | --- | --- |
| {{TRACE_REQUIREMENT}} | {{TRACE_CAPABILITIES}} | {{TRACE_DATA}} | {{TRACE_COMPONENTS}} | {{TRACE_FLOWS}} | {{TRACE_CANDIDATES}} |

## Supporting Material [Optional]

{{OPTIONAL_SUPPORTING_MATERIAL}}

## Approval And Review Checklist [Required]

- [ ] Scope routing, filename, number, metadata, and central index row are correct.
- [ ] Every Trello source has a captured baseline, acceptance outcome, and last-checked date.
- [ ] Every functional capability cites captured requirement IDs, and every stated behavior traces to those cited baselines.
- [ ] When Data Model Design is triggered, ownership, lifecycle, sensitivity, relationships, and invariants are populated; otherwise the section records `N/A — <reason>`.
- [ ] Every architecture component has a responsibility, conceptual inputs and outputs, dependencies, and cited accepted constraints.
- [ ] Every triggered control or interaction section covers success, applicable branches, failure, and recovery; each untriggered section records `N/A — <reason>`.
- [ ] UI scope cites exact accessible Figma context and does not override it.
- [ ] Cross-cutting concerns, risks, and assumptions are documented with their treatment, and every material question is resolved.
- [ ] Traceability connects every requirement to capabilities and ADR task candidates.
- [ ] Task candidates contain outcomes and boundaries but no implementation details.
- [ ] Every `Selected` or `Complete` candidate has an exact ADR path, and that ADR's `Architecture Source` points back to this ADD path and candidate ID.
- [ ] Every required section is complete; every conditional trigger is assessed and completed or marked `N/A — <reason>`; optional sections are complete or removed.
- [ ] An eligible non-author approver, approval time, and exact `Approval Evidence: Approve` are recorded before `Current`; any optional Approval Context Revision is informational, non-binding, and exactly represents the approved document.

## Archival [Conditionally Required — Design Status is `Deprecated` or `Superseded`]

When Design Status becomes `Deprecated` or `Superseded`, archive this ADD in
the same change. Before that trigger, retain this section as inactive
future-lifecycle guidance; its checklist does not affect `Current` status. When
triggered:

- [ ] Confirm every candidate is `Deferred` or `Complete`, no linked ADR has a non-terminal Implementation Status, and all reciprocal paths are current.
- [ ] Move it to `archive/ADD-{{ADD_NUMBER}}-<slug>.md` under this architecture root.
- [ ] Update all ADR, ADD, code, documentation, and task-candidate references to its final path.
- [ ] For supersession, set reciprocal `Supersedes` and `Superseded By` paths.
- [ ] Update its single row in `docs/architecture/INDEX.md`; never delete the row.
- [ ] Confirm no non-archived ADD/ADR or governed marker still cites the pre-archive path.

## Change Log [Required]

| Date | Change | Author |
| --- | --- | --- |
| {{CHANGE_DATE}} | {{CHANGE_DESCRIPTION}} | {{CHANGE_AUTHOR}} |
