# ADR-0016: ADR Rejection Reason

## Metadata [Required]

- **Decision Status**: Accepted
- **Implementation Status**: Complete
- **Date**: 2026-09-22
- **Author**: @codex
- **Decision Owner**: @linhai
- **Required Approver**: @linhai
- **Record Scope**: Project
- **Approver [Conditionally Required — Decision Status is or has been `Accepted`]**: @linhai
- **Approval Time [Conditionally Required — Decision Status is or has been `Accepted`]**: 2026-09-23T14:47:06Z
- **Approval Evidence [Conditionally Required — Decision Status is or has been `Accepted`]**: Approve
- **Rejector [Conditionally Required — Decision Status is `Rejected`]**: N/A — Decision Status is Accepted
- **Rejection Time [Conditionally Required — Decision Status is `Rejected`]**: N/A — Decision Status is Accepted
- **Rejection Evidence [Conditionally Required — Decision Status is `Rejected`]**: N/A — Decision Status is Accepted
- **Rejection Reason [Conditionally Required — Decision Status is `Rejected`]**: N/A — Decision Status is Accepted
- **Retired By [Conditionally Required — Decision Status is `Deprecated` or `Superseded`]**: N/A — Decision Status is Accepted
- **Retirement Time [Conditionally Required — Decision Status is `Deprecated` or `Superseded`]**: N/A — Decision Status is Accepted
- **Retirement Evidence [Conditionally Required — Decision Status is `Deprecated` or `Superseded`]**: N/A — Decision Status is Accepted
- **Retirement Reason [Conditionally Required — Decision Status is `Deprecated` or `Superseded`]**: N/A — Decision Status is Accepted
- **Blocked From [Conditionally Required — Implementation Status is `Blocked`]**: N/A — Implementation Status is Not Started
- **Blocker And Evidence [Conditionally Required — Implementation Status is `Blocked`]**: N/A — Implementation Status is Not Started
- **Blocker Owner [Conditionally Required — Implementation Status is `Blocked`]**: N/A — Implementation Status is Not Started
- **Blocker Exit Or Recheck Criterion [Conditionally Required — Implementation Status is `Blocked`]**: N/A — Implementation Status is Not Started
- **Related [Optional]**: [AWS ADR process](https://docs.aws.amazon.com/prescriptive-guidance/latest/architectural-decision-records/adr-process.html); current Codex task `01a0c834-b8cb-7082-9103-5cd0e65eb3de`, user scope determination on 2026-09-22; [PR #15](https://github.com/hailingu/koduck/pull/15), submission explicitly requested by the user; `docs/adr/ADR-0015-local-sonarqube-feature-completion-gate.md`
- **Architecture Source [Conditionally Required — product demand]**: N/A — repository-governance change requested in the ADR-template assessment, not product demand
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

Both ADR templates and the canonical rejection policy record who rejected an
ADR, when it was rejected, and the exact rejection evidence. They do not require
the reason. A rejected proposal can consequently retain its authorization trail
without explaining why the option was declined.

The user's 2026-09-22 scope determination accepts adding a conditionally required
`Rejection Reason` to Full and Lightweight ADRs. It explicitly declines adding
Lightweight ADR rationale or consequences and declines changing the existing
Accepted-to-Proposed reapproval mechanism. This proposal implements only that
accepted scope, with the minimum supporting validation.

At inspected revision `06445f7a638dd280aa1a16d653b1180b33ee48ee`, all indexed ADRs
had terminal Implementation Status and none had Decision Status `Rejected`.
The blocked OCR does not participate in ADR serialization. A later inspection
on 2026-09-23 found branch `codex/simplify-concurrency-scope` at
`0f1fe3dbac8849c8aebae93741cff5a8628fb534` with a clean worktree before
this revision. Its index still contained no `Rejected` ADR; ADR-0016 was the
sole non-terminal ADR.

## Scope [Required]

In scope:

- The rejection metadata and drafting instructions in the Full and Lightweight
  ADR templates.
- The ADR-specific rejection rule in `AGENTS.md`, its corresponding policy in
  `AGENTS.template.md`, and the navigation summary in `docs/README.md`.
- The governance validator's rejection gate and focused CLI regression tests.
- This record, its index row, and verification evidence for this one outcome.

Out of scope:

- OCR or ADD metadata requirements, rejection authority, approval identity,
  exact `Approve` or `Reject` evidence, reapproval, archival, or serialization
  changes.
- Lightweight ADR rationale, consequences, or alternative-analysis sections.
- Dependencies, CI configuration, SonarQube implementation, bulk historical
  record rewrites, Trello writes, and releases. Submission of this proposal to
  existing PR #15 is separately authorized by the user's subsequent instruction.

## Tensions, Constraints, And Open Questions [Required]

### Identified Tensions [Conditionally Required — competing goals or trade-offs exist]

| ID | Tension | Impact | Decision |
| --- | --- | --- | --- |
| TN-1 | Preserve a rejection explanation without expanding ordinary ADR authoring | Extra mandatory prose in every state would exceed the requested scope | Require the explanation only for Rejected ADRs; new non-Rejected records retain a reasoned N/A value |

### Constraints [Required]

- Keep the user's two declined recommendations outside the change.
- Reuse the existing metadata parser, completeness predicate, record
  classification, and CLI fixture helpers; introduce no dependency or parallel
  Markdown parser.
- Follow `AGENTS.md` Approval and Status, Scope Routing, and source test-first
  requirements, plus the common software-engineering standard.
- Implementation follows this Accepted revision's scope and acceptance checks.

### Open Questions [Conditionally Required — material questions exist or were resolved during drafting]

| ID | Question | Owner | Due | Status | Resolution and Evidence |
| --- | --- | --- | --- | --- | --- |
| Q-1 | Which assessment recommendations should proceed? | @linhai | Before approval | Resolved | The current task's 2026-09-22 user message selects only Rejection Reason for the two ADR templates and explicitly retains the other two existing policies |

## Decision Drivers [Required]

1. **Decision history**: Readers should be able to understand why a proposal was
   rejected without reconstructing an external conversation.
2. **Narrow scope**: The rule applies to the two ADR types the user identified.
3. **Enforcement**: Removing the field entirely must not bypass a triggered
   requirement that the template merely suggests.

## Options Considered [Required]

### Option: Add the field only to the templates

Pros:

- Minimal authoring-document change.

Cons:

- The existing rejection validator would still accept a rejected ADR that
  omits the field, and the canonical policy would not state the new requirement.

### Option: Add the field with matching policy and ADR-only validation

Pros:

- Template guidance, canonical policy, and rejection validation agree.
- Existing metadata parsing and completeness checks provide the implementation.

Cons:

- Requires a small source change and focused regression tests in addition to
  the requested authoring fields.

## Decision [Required]

**Selected option**: Add the field with matching policy and ADR-only validation.

**Rationale**: A conditional requirement is useful only when the authoring
contract and lifecycle gate enforce the same rule. Its application remains
limited to ADRs, preserving the user's scope.

The following clauses define the complete change contract:

- **RR-1 — Authoring field**: Both ADR templates MUST retain
  ``Rejection Reason [Conditionally Required — Decision Status is `Rejected`]``
  in Metadata, using the templates' existing Markdown convention for status
  names. Drafting instructions MUST require a concrete explanation of why the
  proposal was rejected, including the deciding constraint or evidence when
  applicable. Outside that state, new instances retain a reasoned `N/A` value.
- **RR-2 — Rejection gate**: Every Full or Lightweight ADR in `Rejected` state,
  including service-scoped and archived records, MUST supply a complete active
  `Rejection Reason` metadata value. The validator MUST reject missing, empty,
  whitespace-only, `Pending`, `N/A`, or unresolved template-placeholder values
  using the existing `isCompleteValue` semantics. Each missing or incomplete
  reason error MUST produce exit status 1 and one stderr line formatted
  `<path>: ADR_REJECTION_REASON_REQUIRED — <description>`, where `<path>` is the
  repository-relative record path. Tests assert the path and diagnostic code,
  not the explanatory prose. Human review determines whether the explanation
  is substantively sound.
- **RR-3 — Metadata boundary**: Only the unique field in the active Metadata
  section can satisfy RR-2. A value in a code fence, HTML comment, Change Log,
  narrative section, or duplicate active fields MUST NOT satisfy the gate.
- **RR-4 — Compatibility**: This change MUST NOT require Rejection Reason for
  OCRs or ADRs whose Decision Status is not Rejected, nor change the existing
  rejection actor, timestamp, exact evidence, final-state, or archival checks.
  Existing non-Rejected ADRs need no metadata backfill. The two user-declined
  policy changes remain excluded.

### Consequences [Required]

Positive:

- Rejected ADRs retain an explicit explanation alongside their existing audit
  fields, and omission is detected by the existing governance command.

Negative:

- Rejecting an ADR requires one additional metadata value.
- Structural validation cannot judge the factual adequacy of the explanation.

Mitigations:

- Activate the requirement only for rejected ADRs and retain structured human
  review for the explanation's substance.

## Implementation Plan [Required]

**Complete task outcome**: The two ADR templates, canonical policy, and
governance validator consistently require Rejection Reason for rejected ADRs,
with compatibility and parser-boundary behavior verified in one task PR.

**Primary implementation boundary**: Governance-validator ADR lifecycle
validation; policy and template edits state the same directly supporting field
contract.

Allowed subtask statuses: `Not Started`, `In Progress`, `Blocked`, `Complete`,
or `N/A — <specific reason>`.

| ID | Objective or deliverable | Included scope | Status | Actual implementation evidence |
| --- | --- | --- | --- | --- |
| T-1 | Enforce and document RR-1 through RR-4 with focused regressions and routed verification | The affected paths below, delivered as one implementation slice | Complete | Implemented at `3a7dd812614336c451df455e3616c23d6f341fdd`: red observed 2026-09-23 with `node --test test/rejection-reason.test.mjs` in `tools/governance-validator` failing 1/1 — the missing-reason Rejected-ADR fixture exited 0 instead of 1; `validateRejectionMetadata` then gained the ADR-only reason gate, `test/rejection-reason.test.mjs` delivered 24 focused CLI regressions in the RR-2/RR-3/RR-4 groups, and both ADR templates plus `AGENTS.md`, `AGENTS.template.md`, and `docs/README.md` state the conditional field; `npm test --prefix tools/governance-validator` passed 208/208 and `npm run validate --prefix tools/governance-validator` exited 0; SonarQube admission evidence recorded in AC-6 |

**Affected paths**: `AGENTS.md`; `AGENTS.template.md`; `docs/README.md`;
`docs/adr/template/0000-template.md`;
`docs/adr/template/0000-lightweight-template.md`;
`tools/governance-validator/validate.mjs`;
`tools/governance-validator/test/rejection-reason.test.mjs`;
`docs/adr/ADR-0016-adr-rejection-reason.md`; `docs/adr/INDEX.md`.

### Stable Implementation Touchpoints [Conditionally Required — source or configuration implementation]

| Path | Stable symbol or contract anchor | Key code excerpt, when needed | Purpose | Source revision |
| --- | --- | --- | --- | --- |
| tools/governance-validator/validate.mjs | validateTerminalStages; validateRejectionMetadata; isCompleteValue | N/A — stable symbols are sufficient | In validateTerminalStages, keep the existing rejection metadata check for both record types and require a reason only when the path is an ADR; reuse isCompleteValue and the current stderr error collection | 3a7dd812614336c451df455e3616c23d6f341fdd — implemented revision |
| tools/governance-validator/lib/markdown-parsing.mjs | isRecordFilename | N/A — stable symbol is sufficient | Reuse the existing imported filename classifier for the ADR-only branch; no parser change is planned | 0f1fe3dbac8849c8aebae93741cff5a8628fb534 — inspected before this revision; unchanged at 3a7dd81 |
| tools/governance-validator/lib/metadata-validation.mjs | createMetadataValidator | N/A — stable symbol is sufficient | Reuse its active-section reader and duplicate-field behavior without modifying its parser | 06445f7a638dd280aa1a16d653b1180b33ee48ee — inspected baseline; unchanged at 3a7dd81 |
| tools/governance-validator/test/rejection-reason.test.mjs | RR-2, RR-3, RR-4 CLI regression groups | N/A — contract anchors identify the planned tests | Exercise complete repository fixtures through the real validator; reuse test/fixtures.mjs run, validRepository, and write helpers | 3a7dd812614336c451df455e3616c23d6f341fdd — implemented revision |

Before production changes, add the smallest missing-reason fixture regression
and observe that the current validator wrongly accepts it; record that red
command and result in T-1's Actual implementation evidence. Then implement the
ADR-only check and cover the selected matrix cases. Tests assert CLI outcomes
and the new diagnostic contract, never ordinary repository-document wording.
New temporary fixture roots are removed after each focused test; isolate the
full suite in a task-owned temporary directory and remove it after execution.

**Migration and rollback strategy [Conditionally Required — this replaces or
changes existing behavior]**: The inspected index has no Rejected ADR, so no
historical reason is invented or backfilled. Recheck that fact before
implementation; newly encountered rejected records require an authoritative
reason and otherwise block validation. Stop if the new gate affects OCRs or
non-Rejected ADRs. A rollback must revert the policy, templates, and check
together under the applicable authorization; previously recorded factual
rejection explanations can remain as history.

### Engineering Exceptions [Conditionally Required — an engineering rule is exceeded or waived]

N/A — no exception is proposed. Reassessed at implementation revision
`3a7dd81`: validate.mjs has 761 physical lines, above its 600-line review
threshold but below its 800-line exception limit; validateRejectionMetadata
spans 13 lines, within the 80-line hard limit; the new test file spans 354
lines, below its 1000-line review threshold. Extending this existing lifecycle
predicate keeps one owner and avoids a separate parser; the threshold crossing
retains the recorded decomposition rationale of one cohesive CLI validator
entry point. Cyclomatic complexity: N/A — no configured complexity tool;
executable-unit span and nesting review supply the required substitute checks.

### Key State And Invariant Matrix [Required]

The owning boundary for every row is ADR lifecycle validation. Entry points are
the CLI's Full/Lightweight record dispatch and project/service archive discovery.

| ID | Precondition or state | Action or transition | Expected observable outcome | Invariant | Test or verification gap |
| --- | --- | --- | --- | --- | --- |
| S-1 | Otherwise valid archived Rejected Full or Lightweight ADR in either scope | Validate with a missing, blank, whitespace, Pending, N/A, or template-placeholder reason | Exit 1 with path and ADR_REJECTION_REASON_REQUIRED on stderr | Every rejected ADR retains a complete active reason | AC-1 |
| S-2 | Same rejected fixtures with a concrete reason | Validate plain and requirement-labeled metadata fields | Exit 0 | The gate accepts a complete explanation independent of ADR type or scope | AC-2 |
| S-3 | Rejected ADR with reason only in a comment, fence, history, narrative, or duplicated active metadata | Validate through the real Markdown parser | Exit 1 with path and ADR_REJECTION_REASON_REQUIRED on stderr; fake or ambiguous metadata cannot satisfy the reason requirement | Only one active Metadata value supplies the explanation | AC-3 |
| S-4 | Proposed or Accepted ADR without this field; otherwise valid Rejected OCR without this field | Validate each fixture | Exit 0 | The new gate applies only to Rejected ADRs | AC-4 |
| S-5 | Rejected ADR with valid reason but missing existing rejection evidence or wrong implementation status/archive location | Validate after independently invalidating each precondition | Exit 1 for the existing violated lifecycle rule | Adding a reason never substitutes for authorized rejection and archival | AC-4 |
| S-6 | S-1 failure | Add the missing concrete active reason and validate again | Exit 0 | Correcting the input restores validity without state outside the document | AC-2 |

No new asynchronous state, concurrency, retry loop, timer, cancellation handler,
or persistence boundary is introduced. The recovery case is S-6. Substantive
reason adequacy remains a disclosed human-review limit, not a CLI guarantee.

## Contract-To-Check Traceability [Conditionally Required — source or configuration implementation]

| Clause ID | Authoritative contract path and heading | Exact normative requirement | Acceptance check or deterministic test IDs | Explicit coverage method |
| --- | --- | --- | --- | --- |
| RR-1 | docs/adr/ADR-0016-adr-rejection-reason.md — Decision | Both ADR templates and supporting policy express the conditional reason field | AC-5 | Structured contract inspection plus repository governance validation |
| RR-2 | docs/adr/ADR-0016-adr-rejection-reason.md — Decision | Reject missing/incomplete active reasons with a path-prefixed stderr diagnostic and accept concrete reasons for both ADR types and scopes | AC-1, AC-2 | Real CLI fixtures exercise each incomplete category, the path and code format, successful values, and correction after failure |
| RR-3 | docs/adr/ADR-0016-adr-rejection-reason.md — Decision | Only unique active Metadata can supply the reason | AC-3 | Put otherwise concrete reasons in each excluded location and in duplicate fields |
| RR-4 | docs/adr/ADR-0016-adr-rejection-reason.md — Decision | Preserve non-triggered states, OCRs, existing rejection gates, and user scope limits | AC-4, AC-5 | Compatibility fixtures, existing lifecycle regressions, and structured scope review |

## Risk Coverage Matrix [Conditionally Required — source or configuration implementation]

| Risk dimension | Applicability and scenario, or specific N/A reason | Owning boundary | Deterministic verification method | Exact expected result | Acceptance check IDs | Status | Actual evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| concurrency and ordering | N/A — the new rule is a stateless predicate over one parsed document; no scheduling or shared state changes | ADR lifecycle validator | AC-5 structured diff review of the declared touchpoints for scheduling and shared state | No scheduling or shared-state behavior added | AC-5 | N/A — no shared mutable state change | AC-5 diff review at 3a7dd81: the validate.mjs change adds one stateless metadata predicate and one stderr collection line; no scheduling or shared state introduced |
| timeout and deadline | N/A — no timer, deadline, external call, or wait is added | ADR lifecycle validator | AC-5 structured diff review of the declared touchpoints for timers, deadlines, and waits | No timer, deadline, external call, or wait added | AC-5 | N/A — no timing policy change | AC-5 diff review at 3a7dd81: no timer, deadline, external call, or wait appears in the changed lines |
| cancellation and interruption | N/A — the synchronous metadata predicate owns no cancellable operation or cleanup lifecycle | ADR lifecycle validator | AC-5 structured diff review of the declared touchpoints for cancellation and cleanup | No cancellable operation or cleanup lifecycle added | AC-5 | N/A — no cancellation behavior change | AC-5 diff review at 3a7dd81: the synchronous validation loop and its error collection are unchanged in structure |
| resource bounds and backpressure | N/A — reuse existing metadata parsing and completeness checks; no queue, collection, or input-size policy is introduced | ADR lifecycle validator | AC-5 structured diff review of the declared touchpoints for queues, collections, and input-size policy | No queue, collection, or input-size policy added | AC-5 | N/A — no resource policy change | AC-5 diff review at 3a7dd81: the gate reuses metadata(), isCompleteValue, and isRecordFilename without new collections or size policy |
| framework or trust-boundary rejection | Applicable — a rejected record omits its reason or attempts to supply it through non-active or ambiguous metadata | Markdown-to-lifecycle validation boundary | Real CLI fixture regression groups for RR-2 and RR-3 | Exit 1 with path and ADR_REJECTION_REASON_REQUIRED on stderr for invalid input; exit 0 after valid correction | AC-1, AC-2, AC-3 | Pass | 2026-09-23 `node --test tools/governance-validator/test/rejection-reason.test.mjs` at 3a7dd81: 24/24 pass — every invalid fixture exits 1 with the path-prefixed diagnostic, valid and corrected fixtures exit 0 |

## Acceptance Checks [Required]

| Check ID | Subtask | Binary acceptance point | Preconditions or input | Verification method | Exact expected result | Expected evidence | Status | Actual result and evidence |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| AC-1 | T-1 | Incomplete reasons cannot pass a triggered ADR rejection gate | S-1 fixtures, covering both ADR types and both scope roots | node --test tools/governance-validator/test/rejection-reason.test.mjs, RR-2 failure group | Every invalid fixture exits 1 and stderr contains `<path>: ADR_REJECTION_REASON_REQUIRED —` for that fixture's repository-relative path | Focused command result and fixture scenario IDs | Pass | 2026-09-23 at 3a7dd81: focused run 24/24 pass; RR-2 failure group covers missing, empty, whitespace-only, Pending, annotated Pending, N/A, and template-placeholder reasons on Full and Lightweight ADRs in project and service archive roots, each exit 1 with the path-prefixed diagnostic |
| AC-2 | T-1 | Concrete active reasons pass and correcting a missing reason restores validity | S-2 and S-6 otherwise valid fixtures | Same focused test command, RR-2 success and recovery group | Every valid and corrected fixture exits 0 | Focused test report | Pass | 2026-09-23 at 3a7dd81: a plain-field Full ADR and a requirement-labeled Lightweight ADR in a service archive exit 0; the S-6 recovery test first observes exit 1, then only the active reason is added and the same fixture exits 0 |
| AC-3 | T-1 | Non-active or duplicate reason fields cannot satisfy RR-2 | S-3 parser-boundary fixtures | Same focused test command, RR-3 group | Every invalid fixture exits 1 and stderr contains `<path>: ADR_REJECTION_REASON_REQUIRED —`; duplicate metadata also retains its existing diagnostic | Focused test report | Pass | 2026-09-23 at 3a7dd81: reasons placed in an HTML comment, a fenced code block, the Change Log, and a narrative section each exit 1 with the diagnostic; duplicate active fields exit 1 and retain `metadata field Rejection Reason must appear exactly once; found 2` |
| AC-4 | T-1 | The new gate preserves compatibility and existing lifecycle rejection | S-4 and S-5 fixtures and existing governance fixtures | Focused RR-4 group, then npm test --prefix tools/governance-validator | Valid non-triggered fixtures exit 0; existing invalid lifecycle fixtures exit 1; full suite has zero failures | Focused and full-suite reports | Pass | 2026-09-23 at 3a7dd81: Proposed and Accepted ADRs and a Rejected OCR without the field exit 0; a complete reason does not rescue a missing Rejector, a non-Not-Applicable status, or non-archived residence (each exit 1 with its existing diagnostic); full suite 208/208 pass, zero failures |
| AC-5 | T-1 | Authoring and governance documents express RR-1 and RR-4 without scope expansion; four baseline risks remain inapplicable | Implementation diff for the declared affected documents | Inspect metadata semantics, conditional trigger, rejection instructions, both policy guides, and scope; inspect the `validate.mjs` diff for scheduling or shared mutable state, timers or waits or external calls, cancellable operations or cleanup lifecycle, and queues or collections or input-size policy; run npm run validate --prefix tools/governance-validator and git diff --check | Both ADR templates require the field only when Rejected; canonical policy agrees; OCR requirements, Lightweight rationale/consequences, and reapproval rules are unchanged; none of the four risk mechanisms is added; both commands exit 0 | Structured review disposition, diff, and validation output | Pass | 2026-09-23 structured review of the 3a7dd81 diff: both templates add exactly `Rejection Reason [Conditionally Required — Decision Status is `Rejected`]` plus drafting guidance; AGENTS.md/AGENTS.template.md add the ADR-scoped requirement with the OCR exemption; docs/README.md summarizes it; the OCR template, Lightweight sections, and reapproval rules are untouched; the validate.mjs diff adds one stateless predicate with none of the four mechanisms; `npm run validate --prefix tools/governance-validator` and `git diff --check` exited 0 |
| AC-6 | T-1 | Source verification satisfies the configured local admission gate for the implementation revision | Committed implementation source and same-source coverage; owner-authorized scanner environment | python3 tools/sonarqube/gate.py check --revision HEAD | Zero incremental unresolved issues, analysis-bound Quality Gate OK, and at least 80 percent coverage of changed executable lines, or the workflow's proven zero-line case | Exact revision/tree, baseline, analysis identity, and coverage evidence | Pass | 2026-09-23 check at revision `3a7dd812614336c451df455e3616c23d6f341fdd` (tree `c864129c8f895ad51c9948e4f195bb6c9155a163`, base `1c105015602fd7ff0abd218aa5d2bffa732a66b7`): new_issues 0, Quality Gate OK, coverable 0 / covered 0 — the change touches only root `tools/` and documentation paths, which the workflow excludes from the product coverage fraction (proven zero changed product executable lines); pre-commit analysis `5a22af98-489d-4ce4-9df6-a3df17dd2c0d`, check analysis `b0d7e83e-139b-46ec-93eb-af9edf653bf5`, task `e7ed8da9-e5ea-4375-b6a8-fde91821a5a4`; record stored under the Git common directory at `.git/sonarqube/dba152b80f2868c47aa3b3424e5b0a0b9295f541e9ecb75c86ec85ef050c781d.json` |

## Completion Checklist [Required]

| ID | Item | Completion Criterion | Expected Evidence | Status | Actual Evidence |
| --- | --- | --- | --- | --- | --- |
| A-1 | ADR approved | Eligible non-author identity, time, and exact Approval Evidence: Approve recorded | Metadata and approval context | Complete | @linhai approved this revised ADR-0016 in the current task on 2026-09-23; active metadata records the exact evidence and time |
| A-2 | Complete task delivered | T-1 complete and AC-1 through AC-6 Pass with actual evidence | Implementation Plan and Acceptance Checks | Complete | T-1 Complete and AC-1 through AC-6 Pass with actual results recorded at implementation revision `3a7dd812614336c451df455e3616c23d6f341fdd` |
| A-3 | Reciprocal ADD link synchronized, when applicable | N/A — repository governance is not derived from an ADD candidate | Architecture Source assessment | N/A — no ADD candidate | N/A — governance-only scope |
| A-4 | Requirement levels satisfied | Required proposal content complete, conditional triggers assessed, and retained optional content accurate | Structured document review | Complete | Required content and conditional assessments are present; active approval fields are complete; implementation and completion evidence recorded at 3a7dd81 |
| A-5 | Acceptance checks are decidable | Every check identifies T-1, input, method, exact result, and evidence | Structured acceptance-check review | Complete | 2026-09-23 round-3 corrections: AC-1 is rerunnable against current behavior; AC-5 explicitly checks the four N/A risk mechanisms; AC-1 through AC-6 retain binary outcomes |
| A-6 | Engineering exceptions governed, when applicable | No changed unit exceeds an unapproved exception limit or the executable-unit hard limit | Point-in-time source measurements and decomposition review | Complete | Reassessed at 3a7dd81: validate.mjs 761 lines (above the 600-line review threshold, below the 800-line limit, with the recorded decomposition rationale); validateRejectionMetadata 13 lines; rejection-reason.test.mjs 354 test lines under its 1000-line threshold; no unit exceeds an unapproved limit |
| A-7 | Contract and baseline risks covered | RR-1 through RR-4 covered and applicable risk row Pass, with specific N/A reasons for other dimensions | Traceability and risk matrices | Complete | RR-1 through RR-4 map to AC-1..AC-5 in the traceability table; the framework or trust-boundary row is Pass with the 24/24 focused regression evidence; the other four dimensions retain specific N/A reasons with the AC-5 diff review recorded |
| A-8 | Governance validation passed | Repository governance validation exits 0 | npm run validate --prefix tools/governance-validator output | Complete | 2026-09-23: revised proposal and index validation exited 0; git diff --check exited 0; the earlier 184/184 suite baseline was repeated against the implementation as 208/208 pass at 3a7dd81, and governance validation exited 0 again after the evidence updates in this change |

## Supporting Notes [Optional]

The Decision Owner and Required Approver name the repository-governance owner
already identified by the linked Accepted ADR-0015. @linhai self-declared in
the current task before explicitly approving ADR-0016; the machine login was
not used as approval identity. That approval was invalidated when the planned
Risk Coverage Matrix columns were completed to satisfy the Accepted-stage gate.
@linhai then reapproved that revision in the same task; the current round-3
contract corrections invalidated that approval and were approved again at
2026-09-23T14:47:06Z.

The source implementation must follow the existing review-ready and required-CI
gates. The user separately requested submission of the proposal to existing
PR #15; that instruction did not itself accept the ADR or authorize implementation.
CI requirements remain unchanged; local SonarQube remains the existing
CI-correspondence exception.

Review evidence: round 1 was the read-only template assessment at
`06445f7a638dd280aa1a16d653b1180b33ee48ee`; round 2 was the proposal's
structured follow-up at the same base on 2026-09-22. No revision had been
pushed at those reviews. Round 3 was @linhai's 2026-09-23 read-only review of
staged ADR and INDEX content based on HEAD
`ced9af96dec6b0e47481b747a7219be271368ad5`. That snapshot was accurate
at review time. The staged changes were then committed as
`0f1fe3dbac8849c8aebae93741cff5a8628fb534` at 2026-09-23T14:23:56Z;
its parent is `ced9af9` and its 55-line ADR / 2-line INDEX change statistics
match the recorded staged diff. The same task's self-declared @linhai directed
correction of the listed findings, authorizing round-3 remediation. Findings
1–4 exposed contract and acceptance gaps and triggered reapproval; findings
5–6 identified touchpoint and owner metadata inaccuracies; finding 7
identified stale context evidence. Finding 8 concerned a status statement in
the separately Current ADD-0001; the wording matched during the Proposed
revision but is stale again after this approval. That document was left
unchanged pending its owner's scope determination.

Round 4 is @linhai's subsequent 2026-09-23 read-only review of this uncommitted
proposal based on HEAD `0f1fe3d`, ADR working-tree blob
`ecd003c61fa1aa063aac91bea80ed7d6adeb19cf`, and INDEX working-tree blob
`c5b24a5b819cc3e0514c73992f1727e92390ee72`. The same task's owner request
authorizes one evidence-only remediation round for its two findings: record
this round and correct the round-3 timeline. The review confirmed the ADR-only
OCR branch, unique active Metadata reading, and no historical backfill need.
The optional touchpoint-baseline normalization is deferred because both cited
revisions remain accurate; the four N/A risk rows will receive actual AC-5
evidence at implementation completion. Round 5 requires separate owner
authorization. Neither review approves this revision or verifies
implementation. Any later pushed revision still requires its own
revision-bound review coverage under `AGENTS.md`.

## Archival [Conditionally Required — Decision Status is `Rejected`, or Decision Status is `Deprecated` or `Superseded` and Implementation Status is final]

Inactive future-lifecycle guidance until the stated trigger applies. At that
time, complete the canonical rejection or retirement metadata and truthful final
Implementation Status, move this record to `docs/adr/archive/` with its filename
retained, update every live reference and code marker, update this index row,
and preserve reciprocal replacement paths when superseded. Verify no live record
or code marker cites the old path. Retain Superseded By: None without a replacement.

## Change Log [Required]

| Date | Change | Author |
| --- | --- | --- |
| 2026-09-22 | Proposed only the user-selected conditional ADR rejection-reason requirement; retained the two declined policy changes outside scope; implementation has not begun | @codex |
| 2026-09-23 | Accepted after @linhai explicitly approved ADR-0016 in the current task; implementation remains Not Started | @linhai |
| 2026-09-23 | Reset to Proposed at 2026-09-23T14:14:22Z after approval-invalidating completion of four Risk Coverage Matrix planned rows; prior Approver: @linhai; Approval Time: 2026-09-23T14:12:30Z; Approval Evidence: Approve; no Approval Context Revision was recorded; reapproval required | @codex |
| 2026-09-23 | Accepted after @linhai reapproved the revised ADR-0016 in the current task; implementation remains Not Started | @linhai |
| 2026-09-23 | Reset to Proposed at 2026-09-23T14:29:48Z for corrections to RR-1, RR-2, Risk Coverage Matrix, and AC-1/AC-5 after round-3 findings; prior Approver: @linhai; Approval Time: 2026-09-23T14:22:55Z; Approval Evidence: Approve; no Approval Context Revision was recorded; Implementation Status remains Not Started | @codex |
| 2026-09-23 | Corrected the round-3 staged-review chronology and recorded @linhai's bounded round-4 evidence review; decision, checks, and Proposed / Not Started statuses are unchanged | @codex |
| 2026-09-23 | Accepted after @linhai approved the revised ADR-0016 in the current task; implementation remains Not Started | @linhai |
| 2026-09-23 | Implementation started after user instruction: validator gate, focused CLI regressions, both ADR templates, AGENTS.md/AGENTS.template.md policy, and docs/README.md summary delivered as one slice; T-1 moved to In Progress | @zcode |
| 2026-09-23 | Implementation completed at 3a7dd812614336c451df455e3616c23d6f341fdd: T-1 Complete, AC-1 through AC-6 Pass, framework risk row Pass, exceptions reassessed; Implementation Status set to Complete | @zcode |
