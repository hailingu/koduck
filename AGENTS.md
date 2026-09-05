<!-- ADR: docs/adr/ADR-0001-provider-neutral-turn-kernel.md -->
<!-- ADR: docs/adr/ADR-0015-local-sonarqube-feature-completion-gate.md -->
# Koduck Agent Guide

> Language: English

Koduck is a multi-service project under construction; this repository
currently contains its governance scaffolding, decision-record templates, and
development standards for future services.

This guide applies to the repository root and every descendant path unless a
closer `AGENTS.md` adds stricter path-specific rules. External system,
developer, and user instructions retain their normal precedence. Accepted ADRs
and security or contract documents remain authoritative for their decisions;
this guide routes agents to them and governs day-to-day repository work. When
rules conflict or authority is unclear, stop before mutation and ask the
repository owner.

`MUST` and `MUST NOT` are mandatory. `SHOULD` identifies the expected default
and requires a stated reason to deviate. `MAY` is optional. Examples are
informative and never override a rule.

## Terminology

- **Architecture Design Document (ADD)**: A non-authorizing solution and
  planning document that translates a captured requirement baseline into
  capabilities, data, architecture, flows, constraints, traceability, and
  future ADR task candidates.
- **Architecture Decision Record (ADR)**: A Full or Lightweight record that
  authorizes one implementation decision after acceptance.
- **Operational Change Record (OCR)**: A record that authorizes one reversible
  build, release, deployment, rollback, or runbook operation after acceptance.
- **Decision record**: An ADR or OCR. An ADD is a design document, not a
  decision record.
- **Disposable Verification Execution**: Running existing tests, static or
  formatting checks in check-only mode, type checks, or a local verification
  compile solely to evaluate the current source. It produces no reusable or
  promotable artifact, does not mutate a running, shared, or external system,
  and its disposable compiler or test output is deleted before task completion;
  command output or a test report MAY be retained as verification evidence.
- **Governed Build**: A build that intentionally creates or transforms an
  artifact retained for reuse, distribution, publication, promotion, runtime
  loading, deployment, or a later operational step. For decision-record
  classification, an unqualified build operation means a Governed Build;
  incidental compilation during Disposable Verification Execution does not.
- **Implementation boundary**: One independently failing and reviewable owner
  of behavior, such as domain or application policy, presentation or framework
  delivery, provider or transport integration, persistence or data behavior,
  or runtime assembly. A reviewable implementation slice has exactly one
  primary implementation boundary; supporting interface changes in adjacent
  boundaries MUST remain limited to what that slice requires.
- **`Pending`**: A temporary placeholder showing that a field is deliberately
  incomplete at a lifecycle stage that permits incompleteness. It is valid only
  where a template permits it for the current stage and MUST be replaced with
  actual content or an allowed `N/A — <reason>` / `None — <reason>` before the
  field becomes required by a status or gate. It is never final evidence.

## Non-Negotiable Gates

### Core Rules

- Preserve user and other-task changes. Do not overwrite, reformat, stage, or
  clean unrelated work.
- Never hard-code, expose, or log secrets, tokens, passwords, private keys, or
  sensitive user data.
- Follow the canonical Approval and Status section for document acceptance,
  rejection, actor identity, and reapproval. Any implementation requiring a
  decision record MUST wait until that record is explicitly `Accepted` under
  that contract.
- Use the narrowest change that satisfies the approved scope. Do not add
  dependencies, change contracts, or refactor adjacent code without explicit
  scope.
- Before writing, modifying, or reviewing source code or infrastructure
  configuration, first read `docs/README.md` and every matching
  language or platform standard listed there in full. When a change spans
  multiple languages or platforms, read every matching standard.
- Before planning or performing a release or creating, pushing, changing, or
  deleting a Git tag, read `docs/delivery/releases.md` and
  `docs/delivery/git-tags.md` in full. These standards do not waive the
  decision-record, approval, or external-write gates.
- Inspect the affected code, applicable standards, and existing internal
  capability before implementation. Reuse a suitable component, client,
  helper, schema, or script.
- New public files, modules, classes, components, hooks, types, functions, and
  methods MUST receive intent-bearing documentation before their
  implementation bodies.
- Use explicit error handling and structured diagnostics supported by the
  project; do not conceal failures.
- Do not hard-code environment-specific values or modify generated or vendored
  content unless the configured workflow explicitly requires it.
- Run non-interactive checks for every affected path and report commands,
  results, and anything not run.

### Design Source Of Truth

For frontend Web and native-client UI implementation or design review, the
relevant Figma file or node is the sole source of truth for visual and
interaction design, including layout, components, states, spacing, typography,
color, assets, responsive or adaptive behavior, and motion. The task, ADR, or
PR MUST identify the specific accessible Figma file or node before
implementation or design review begins. Screenshots, prose, and existing code
are supporting context only and MUST NOT override Figma on design decisions.

Figma does not override product behavior, API or data contracts, security,
accessibility, or platform technical constraints. If the relevant Figma context
is missing, inaccessible, ambiguous, internally inconsistent, or conflicts with
another binding requirement, stop and obtain a recorded determination from the
specifically identified human decision owner. Update the appropriate
authoritative artifact before work continues; do not guess the resolution.

### Work Coordination, Review, And Delivery Sources Of Truth

The [Koduck Trello board](https://trello.com/b/Kz9qnd3D/koduck) is the mutable
coordination view for demand, priority, ownership, due dates, progress,
blockers, and discussion. A card MAY link to an ADD, ADR, OCR, branch, commit, or
pull request, but the card body, comments, labels, list position, closure, and
deletion do not by themselves approve, make `Current`, accept, verify,
deprecate, supersede, archive, or delete an ADD, ADR, or OCR.

When an automatic-review mechanism is configured for this repository, its
result is a revision-bound review gate, not formal ADD approval, ADR/OCR
acceptance, or proof of build or deployment; a new push then requires coverage
for the new revision. When no such mechanism is configured, no automatic-review
gate applies and its absence MUST be reported as `N/A — no automatic-review
mechanism configured`. Automatic review never approves or rejects a document;
the canonical Approval and Status section governs those actions.
Record the reviewed commit SHA and result for every pushed revision that seeks
review or merge in the pull request's verification section, or in the governing
ADR/OCR evidence when no pull request exists; when neither exists, record it in
the final task report. A later push invalidates that revision's review coverage.

A source or configuration pull request MUST remain Draft while it obtains
automatic review and MUST NOT be declared review-ready until all of the
following are true for its latest pushed commit:

- every Scope Routing verification command for the affected paths has a
  corresponding required CI check, and every such check is green; absence of
  required CI is a blocker, not `N/A`;
- every applicable contract clause is mapped to a passing acceptance check or
  deterministic test through the governing ADR's contract traceability;
- every applicable row in the governing ADR's Risk Coverage Matrix is `Pass`,
  and every non-applicable row has a specific `N/A` reason;
- when automatic review is configured, it has reviewed that exact commit; when
  none is configured, record the canonical `N/A` result; and
- no actionable, non-outdated P0/P1/P2 or equivalent blocking thread from any
  configured review is unresolved. A disputed finding is unresolved until an
  evidence-backed disposition is recorded and the thread is resolved.

Any later push removes review-ready status and requires the full gate again.
Automatic review supplements rather than replaces CI, deterministic checks,
contract traceability, or human review.

When responding to a GitHub inline review comment, reply through `Reply` in the
original review thread shown beside the affected diff. Every actionable or
disputed thread MUST receive its own thread reply; a top-level pull-request
comment, commit message, reaction, reply in another thread, or evidence recorded
only in an ADR does not count. The reply MUST state whether the finding was
addressed or disputed. An addressed finding cites the fixing commit SHA and
relevant checks; a disputed finding cites the reviewed commit SHA and stable
evidence supporting the disposition. Post the reply before selecting
`Resolve conversation`. Resolve only after the cited revision is pushed and
the thread reply demonstrates one of these outcomes: the finding is fixed, or
an evidence-backed disposition is recorded. Never resolve a thread solely
because it became outdated or received a reaction.

When Trello is used under an Accepted workflow policy or the bootstrap
authorization below, synchronize authoritative Git and ADD/ADR/OCR transitions
outward to Trello. Automation MUST be idempotent and MUST NOT advance Git or a
record because a card was moved, closed, relabeled, or commented on. On
conflict, repository state wins; report the drift and restore or flag the card
without erasing human discussion. Never copy secrets, kubeconfig content,
private endpoints, credentials, or sensitive runtime data into Trello or
review output.

Until the first applicable Trello workflow policy is `Accepted`, an explicit
instruction from the user acting as repository owner authorizes only the
identified routine coordination-metadata actions for the identified task or
cards. It does not authorize board-structure, field-semantics, connector,
automation, or repository-state changes. Record that instruction in the task
context and in a durable location associated with the action: the affected
card's comment or description, a linked pull-request body, or a linked
ADD/ADR/OCR evidence field. The durable entry MUST identify the authorizing
`@<actor-id>`, time, authorized action, and task or conversation reference,
without copying sensitive content. Once an applicable workflow policy is
`Accepted`, use it instead of this bootstrap clause unless the repository owner
explicitly directs a different in-scope action.

#### Review Rounds And Convergence

These rules bound agent-driven review and remediation, not human review or
the required revision-bound gates above. They do not weaken CI, acceptance
checks, risk coverage, SonarQube, or document approval requirements.

- For one task or pull request, agents MUST run at most two review rounds
  without further repository-owner authorization: the initial review and one
  follow-up. A round is one review pass over an identified revision, including
  the results from its configured reviewers. Fixing, pushing, or resuming the
  same work MUST NOT reset the count. Record the round number, reviewed
  revision (the commit SHA for pushed work), and result alongside the
  revision-bound review evidence above.
- If further review or iterative remediation is needed after two rounds,
  pause the automatic review/fix loop and ask the repository owner for an
  explicit, bounded extension or another in-scope disposition. Summarize the
  remaining findings, evidence, risks, and proposed next steps. Budget
  exhaustion MUST NOT itself close a finding, waive a required check, or
  establish completion or review-ready status. Where automatic review is
  configured, any fix pushed after the final round still requires review of
  that exact revision; until authorized review supplies it, the revision
  remains without review coverage. The existing no-automatic-review `N/A`
  rule remains unchanged. Required checks may still run, and newly arriving
  findings MUST be reported rather than discarded because the budget is
  exhausted.
- Deduplicate findings by the underlying defect and violated contract, not
  merely by code location. A repeated finding with no new evidence SHOULD
  reference its existing disposition. An additional finding MUST identify
  a distinct trigger, violated contract, new evidence, or a regression
  introduced by the fix before it justifies reopening settled work. A genuine
  new P0/P1/P2 finding remains actionable even at the same location or after
  the round budget is exhausted.
- For bulk findings, first classify and summarize confirmed defects,
  pre-existing issues, out-of-scope improvements, duplicates, and disputed
  findings. Continue necessary fixes within already-authorized scope; ask the
  repository owner before expanding scope, accepting risk, or changing a
  contract. Do not automatically implement every suggestion or require a new
  human choice for every already-authorized fix. Calling an issue pre-existing
  or out of scope does not by itself waive a blocking finding.
- Record each decision not to change code with its location or stable symbol,
  reviewed revision, reason, and supporting evidence in the pull request or
  linked tracking item. Every actionable or disputed inline thread still
  requires its own reply under the original-thread rule above; a summary or
  tracking entry alone does not permit resolution. An actual engineering
  exception or approval-invalidating change must use its governing approval
  process, not merely an owner comment accepting the review response.
- Do not add suppression comments or exemption markers to code solely to
  dismiss review findings. Existing required governed-file markers remain
  mandatory. Traceable dispositions belong in review evidence and, when
  required, the governing decision record.

### Change Classification

Classify requested work before editing:

| Class | Includes |
| --- | --- |
| Read-only | Investigation, search, review, Q&A — no writes |
| Editorial Documentation | Markdown with no governance, contract, security, deployment, or process meaning |
| Architecture Design Documentation | A repository-versioned solution view derived from Trello requirements, covering capabilities, data, architecture, control flows, interaction flows, and future ADR task candidates without authorizing or specifying implementation |
| Normative / Governance Documentation | Markdown or instructions that define governance, contracts, security, deployment, or process behavior |
| Coordination Metadata | Routine Trello card creation, assignment, linking, labeling, commenting, or state synchronization under an Accepted workflow policy |
| Source | Application, library, or test code |
| Configuration | Build, CI, deployment, environment, or infrastructure config |
| Verification Execution | Disposable Verification Execution as defined above |
| Operational | A Governed Build or a reversible release/deploy/rollback/runbook action against a running, shared, external, or artifact-producing system |

Read-only work needs no decision record. Routine SonarQube hook installation,
disposable verification databases,
and analysis through `tools/sonarqube/gate.py` require no ADR, OCR or repeated
`Approve`, under the owner authorization recorded in
`tools/sonarqube/README.md`. This is a specific operational exception, not a
reclassification of all external writes as Disposable Verification Execution.
A direct, source-only remediation of a
SonarQube-reported code issue — including its focused regression test — needs
no ADR, OCR, or `Approve`; it may be implemented immediately. This exception
applies to issues in every SonarQube software-quality category, but does not
extend to configuration, dependency, deployment, data, public-contract, or
governance changes required alongside the remediation; classify those changes
normally. Drafting or updating an Architecture
Design Document (ADD) needs no prior ADR because an ADD is a non-authorizing
solution and planning artifact; implementation still requires an Accepted ADR
selected from the ADD as defined below. Normative/governance documentation,
source, configuration, and operational work are gated by the Decision Records
policy below. Editorial documentation is eligible for No record only when its
diff proves it has no normative meaning. Routine Coordination Metadata needs no
per-action ADR or OCR when performed under an Accepted workflow policy or the
explicit bootstrap authorization above, but a change to board structure, field
or label semantics, authority mappings, connector permissions, automation
behavior, or failure handling is repository governance and requires a
project-level Full ADR. Changing the ADD workflow, authority, status model,
routing, or template contract is also repository governance rather than an
ordinary ADD update. Disposable Verification Execution needs no decision record
and no additional `Approve`; when it verifies implementation governed by an
ADR, that ADR must already authorize the source change, but running the
verification does not require an OCR. If any disposable-verification condition
is false, classify the action under the highest applicable class instead.

Test-driven development applies to source-code features and reproducible defect
fixes: write the smallest failing behavior or regression test before the
production-code change, observe the expected failure, then implement and keep
the suite green. A pure ADR, ADD, `AGENTS.md`, template, index, or other
documentation-only change does not require a Red-Green-Refactor cycle. It MUST
instead pass the configured deterministic governance validation plus the
applicable structured review. A change to the governance validator itself is
source work and follows test-driven development.

Tests MUST NOT read repository-versioned source or documentation as opaque text
and assert ordinary prose, exact phrasing, substring presence or occurrence
counts, physical line counts, formatting, section placement, or implementation
layout. Verify semantics through the language/compiler or the configured parser
or validator. An exact-text assertion is permitted only when that textual form
is itself an authoritative contract, such as a stable clause ID, required
heading, wire or golden fixture, schema token, command contract, or
machine-readable diagnostic code; the test MUST cite that contract. A fixture
created solely to exercise a parser or validator MAY contain the exact text
needed to represent its grammar, but its assertions SHOULD target semantic
outcomes or stable diagnostic codes instead of ordinary wording.

### Document Requirement Levels

Every ADD, ADR, and OCR template and instantiated document MUST distinguish
content with these exact requirement levels:

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

Templates MUST label top-level sections, identify every conditional trigger,
and include the legend above in instantiated documents. `Pending` is permitted
only while the document's current lifecycle stage genuinely allows incomplete
content. An ADD MUST NOT become `Current`, an ADR or OCR MUST NOT become
`Accepted`, and no record may become `Complete` or `Verified` while any
`[Required]` or triggered `[Conditionally Required]` content for that stage is
missing, blank, unresolved, or still `Pending`. Review checklists MUST verify
requirement-level compliance.

### Architecture Design Documents

#### Purpose, Authority, And Content Boundary

- An ADD translates one or more linked Trello requirements into a coherent,
  repository-versioned solution view. It MUST cover the applicable functional
  capabilities, conceptual or logical data model, system architecture, control
  flows, interaction flows, cross-cutting constraints, traceability, and ADR
  task candidates.
- The required Architecture Design section MUST include at least one fenced
  Mermaid `flowchart` representing every component ID and the applicable system
  boundaries, dependencies, and conceptual data, event, or control directions.
  Its structured component table and Mermaid diagram MUST agree; neither may
  substitute for the other. A missing architecture diagram blocks the ADD from
  becoming `Current`.
- Every triggered Control Flow Design section MUST include at least one fenced
  Mermaid `flowchart` or `sequenceDiagram` in addition to its structured flow
  table. The diagram MUST represent every flow ID in that section and show the
  applicable ordering, branches, retries, failure termination, and recovery.
  Every triggered Interaction Flow Design section MUST likewise include at
  least one fenced Mermaid `sequenceDiagram` or `stateDiagram-v2` representing
  every interaction ID and its actors, actions or transitions, system feedback,
  cancellation, failure, and recovery. Tables and diagrams MUST agree; neither
  may substitute for the other. A triggered section without its required
  diagram blocks the ADD from becoming `Current`.
- An ADD MUST NOT authorize source, configuration, build, deployment, or other
  operational changes. It MUST NOT contain task-level implementation designs,
  file-by-file change plans, code or schema definitions, executable commands,
  or test implementations. Those details belong to the ADR selected for one
  task candidate.
- Trello is the mutable demand source for an ADD, not approval evidence or a
  repository decision authority. Every ADD MUST link each source card, capture
  the requirement and acceptance-outcome baseline used for the design, and
  record when that baseline was last checked. Later card edits do not silently
  modify the ADD; report drift and revise the versioned document deliberately.
- Reading, triaging, and routing a Trello requirement are read-only and MAY
  occur before branch creation. Before copying the ADD template, assigning an
  ADD number, creating the ADD file, or adding its index row for a new ADD, the
  agent MUST inspect the worktree and create and switch to a new task branch
  from the current local `dev`, dedicated to that ADD under the Branch, Pull
  Request, And Commit Policy. `main` MUST NOT be used as its branch point. This
  requirement applies even when the current non-protected branch is otherwise
  authorized. If unrelated work prevents a safe branch switch, stop before
  drafting; do not stash, commit, or move that work without authorization.
- An ADD may propose architecture choices, but those choices become binding for
  implementation only through an Accepted ADR. Existing Accepted ADRs,
  security and contract documents, and platform constraints remain
  authoritative and MUST be cited as constraints.
- When Web or native UI is in scope, the ADD MUST cite the specific accessible
  Figma file or node. Its interaction-flow section may describe actors, states,
  transitions, and system feedback in the required Mermaid diagram, but MUST
  NOT override Figma's visual or interaction decisions. Missing or ambiguous
  Figma context blocks the ADD from becoming `Current` for that UI scope.

#### Scope Routing, Identity, And Storage

Every ADD belongs to exactly one architecture root. Route it with this order of
precedence and stop at the first matching row:

| Signal | Scope level | Root |
| --- | --- | --- |
| Crosses more than one project, service, or package boundary | Repository / Cross-project | `docs/architecture/` |
| Defines a shared capability, data model, control flow, interaction flow, or contract used outside one service or package | Repository / Cross-project | `docs/architecture/` |
| Affects only one service or package internally and changes no externally consumed contract | Service / Package internal | `<service-or-package>/docs/architecture/` |
| Still ambiguous | Repository / Cross-project | `docs/architecture/` |

- ADD filenames are `ADD-NNNN-<slug>.md`. `NNNN` is an increasing decimal
  number unique within its own architecture root, padded with leading zeroes
  to a minimum width of four digits (`0001` through `9999`, then `10000` and
  higher without truncation). The record identity is its repository-relative
  path, never the bare number.
- The shared template is `docs/architecture/template/0000-add-template.md`.
- `docs/architecture/INDEX.md` is the single repository-wide index for active
  and archived ADDs at every scope. Update its one row whenever the title,
  Design Status, scope level, scope, path, Trello source, or supersession field
  changes. Rows are never deleted.
- Design Status is `Draft`, `Current`, `Deprecated`, or `Superseded`. `Current`
  requires resolved material questions, traceability to the captured Trello
  baseline, completed approval metadata, and `Approval Evidence: Approve`. An
  approval-invalidating ADD change, as defined under Approval and Status,
  returns the ADD to `Draft` until an eligible approver responds `Approve`
  again.
- Legal ADD Design Status transitions are `Draft` to `Current` by approval,
  `Current` to `Draft` after an approval-invalidating change, `Draft` or
  `Current` to `Deprecated` by retirement, `Draft` or `Current` to `Superseded`
  by retirement when a replacement is already `Current`, and `Deprecated` to
  `Superseded` when a replacement is identified later. `Superseded` is
  terminal; `Deprecated` may only become `Superseded`. A retired ADD is not
  reactivated — create a new ADD instead.
- Before retiring an ADD, every task candidate MUST be `Deferred` or `Complete`,
  and no ADR that names that ADD in `Architecture Source` may have a non-terminal
  Implementation Status. Move every `Ready` candidate to `Deferred` with the
  retirement reason. For every `Selected` candidate, first reject its Proposed
  ADR or retire its Accepted ADR to `Not Applicable` and move the candidate to
  `Deferred`, or finish the ADR and move the candidate to `Complete`. Update
  every reciprocal path in those changes.
- Archive a `Deprecated` or `Superseded` ADD under `archive/` in its own
  architecture root, retain its filename, update all references and its index
  row, and preserve reciprocal `Supersedes` / `Superseded By` paths when a
  replacement exists.

#### ADD-To-ADR Handoff

- Each ADD task candidate MUST have a stable ID, one complete outcome, scope
  boundaries, dependencies, acceptance context, recommended ADR type, status,
  and eventual ADR path. It describes what must be achieved, not how to
  implement it.
- Each candidate intended for source or configuration implementation MUST fit
  one independently reviewable implementation pull request under the ADR scope
  rule and identify one primary implementation boundary. If it contains
  multiple implementation pull requests, multiple primary implementation
  boundaries, or independently mergeable outcomes, split it into
  dependency-ordered candidates while the ADD is `Draft`, then obtain `Approve`
  before making the revised ADD `Current` or selecting any resulting candidate.
  This rule applies to a candidate first added or materially changed under this
  rule and does not retroactively invalidate an unchanged candidate in an
  already `Current` ADD.
- The complete ADD task-candidate status set is `Ready`, `Selected`, `Complete`,
  and `Deferred`. `Ready` means the candidate is fully specified and eligible
  for selection while its ADD is `Current`; `Selected` requires one reciprocal
  ADR link whose Decision Status is `Proposed` or `Accepted` and whose
  Implementation Status is `Not Started`, `In Progress`, or `Blocked`;
  `Complete` requires that linked ADR to be `Complete` or `Verified`; and
  `Deferred` requires a recorded reason and is not eligible for selection.
  Legal transitions are `Ready` to `Selected`, `Selected` to
  `Complete`, `Selected` to `Ready` or `Deferred` when the linked ADR becomes
  `Not Applicable`, and `Ready` to or from `Deferred` with a recorded reason.
  No other candidate status or transition is permitted.
- A direct `Selected` to `Selected` relink is prohibited. To replace or defer
  selected work, first reject a Proposed linked ADR or retire an Accepted
  linked ADR to `Not Applicable`, move the candidate to `Ready` or `Deferred`,
  and only then select a replacement ADR. If the linked ADR is already
  `Complete` or `Verified`, the candidate becomes `Complete`; any replacement
  work requires a new candidate so completed traceability is not rewritten.
- A source or configuration ADR derived from product demand MUST select exactly
  one `Ready` task candidate from one `Current` ADD and cite the ADD's exact
  repository-relative path plus candidate ID. The ADR owns detailed design,
  its one-to-three subtasks, implementation, verification, and stable evidence.
- The ADD-to-ADR link MUST be reciprocal and created atomically in the same
  change that generates the ADR: the ADD candidate records the ADR's exact
  repository-relative path and becomes `Selected`, while the ADR's
  `Architecture Source` records the ADD's exact repository-relative path and
  candidate ID. An ADR derived from product demand MUST NOT become `Accepted`
  while either side is missing, stale, or disagrees with the other.
- One task candidate MUST NOT link to more than one ADR whose Decision Status
  is `Proposed` or `Accepted` and whose Implementation Status is `Not Started`,
  `In Progress`, or `Blocked`. Mark the candidate `Complete` only when its ADR
  reaches `Complete` or `Verified`. If the ADR becomes `Not Applicable`, return
  the candidate to `Ready` or mark it `Deferred` with a reason and remove or
  retain the ADR path with an explicit historical note.
- Any ADR or ADD rename, move, archival, supersession, or relevant status
  change MUST update both directions in the same change. If both documents
  cannot be updated together, stop before mutation rather than leaving a
  one-way or dangling reference.
- Governance, process, and other ADRs not derived from product demand MAY use
  `Architecture Source: N/A — <reason>`. They remain subject to all normal ADR
  classification, serialization, approval, and evidence gates.
- ADDs may be created or revised while an ADR is unfinished because they do not
  authorize implementation. The repository-wide ADR serialization gate still
  determines when another ADR may be generated.

### Decision Records

#### Classification

| Class | Use when | Gate |
| --- | --- | --- |
| Full ADR | Architecture, governance, cross-service behavior, public API/schema/protocol, security, data, dependency, or build/release/deployment strategy, pipeline, configuration, service-boundary, or irreversible decisions | Accepted before implementation |
| Lightweight ADR | A localized, reversible source behavior change whose checklist proves no Full ADR concern applies | Accepted before implementation |
| Operational Change Record (OCR) | A Governed Build, release, deployment, rollback, or existing runbook operation within an accepted architecture, pipeline, artifact contract, and security/data boundary | Accepted before the operation |
| No record | Read-only work, Disposable Verification Execution, non-normative editorial documentation, routine coordination metadata under an Accepted policy, the owner-authorized SonarQube hook workflow, a direct source-only SonarQube code remediation with its focused regression test, or a provably semantics-neutral formatting/comment-only edit | No decision-record gate; direct SonarQube remediation requires neither ADR/OCR nor `Approve`; normal verification still applies |

Direct source-only remediation of a SonarQube-reported code issue requires no
ADR, OCR, or `Approve`, including issues categorized as Security, Reliability,
or Maintainability. The exception does not cover accompanying configuration,
dependency, deployment, data, public-contract, or governance changes; those
changes remain subject to their normal classification and gates. Normative
Markdown about governance, contracts, security, deployment, or
process is not editorial. A formatting/comment-only edit qualifies only when
its diff proves there is no semantic change. Test source, snapshots, generated
artifacts, lock files, dependencies, config, and mixed-scope changes are not
automatically exempt: classify them by the behavior or decision they support,
using the highest applicable class. Executing existing tests is exempt from an
OCR only when it satisfies every Disposable Verification Execution condition.

Use `docs/adr/template/0000-template.md` for Full ADRs,
`docs/adr/template/0000-lightweight-template.md` for eligible Lightweight
ADRs, and `docs/adr/template/0000-operational-change-template.md` for eligible
OCRs.

#### Task Completeness, Risk Coverage, And ADR Serialization

- Every ADR and OCR MUST define exactly one complete, end-to-end task with an
  objectively verifiable outcome. A record MUST NOT serve as an umbrella for a
  backlog, program, or unrelated set of deliverables.
- A source or configuration ADR first drafted under this rule MUST authorize
  one independently reviewable implementation slice deliverable through one
  implementation pull request. Its one to three subtasks may separate tightly
  coupled implementation, migration, or verification work, but MUST NOT hide
  multiple primary implementation boundaries or unrelated deliverables in one
  ADR. The ADR MUST name its one primary implementation boundary. If an ADD
  candidate requires multiple implementation pull requests, multiple primary
  implementation boundaries, or independently mergeable outcomes, split and
  reapprove the ADD candidates before selecting an ADR. This scope rule does
  not retroactively invalidate an already `Accepted` ADR; an
  approval-invalidating scope change to that ADR MUST comply before reapproval.
- Each record MUST decompose its task into at least one and at most three
  implementation or operational subtasks. Each subtask MUST state its
  objective or deliverable, included scope or target, status, and actual
  implementation or operational evidence before the record may reach
  `Complete` or `Verified`.
- Subtask Status is `Not Started`, `In Progress`, `Blocked`, `Complete`, or
  `N/A — <specific reason>`. When a `Blocked` subtask prevents task progress,
  the record MUST enter `Blocked` and the record-level blocker fields apply. An
  ADR or OCR may become `Complete` or `Verified` only when every subtask is
  `Complete` or has a valid `N/A` reason and the final statuses still satisfy
  the complete task outcome.
- Every ADR MUST define its acceptance checks before approval. Each check MUST
  reference one declared subtask and state one binary pass/fail acceptance
  point, its preconditions or input, an executable or deterministic verification
  method, the exact observable expected result, and the evidence to capture.
  Use a numeric threshold, exact state, response, invariant, or cited contract
  clause whenever the result can vary.
- Every source or configuration ADR first drafted or changed in an
  approval-invalidating way under this rule MUST identify its stable
  implementation touchpoints. Use a repository-relative path plus a fully
  qualified function, method, type, module, configuration key, schema object,
  route, table, or contract-clause name. When no stable symbol or anchor can
  express the decisive constraint, include the shortest key code excerpt that
  can. Record the source revision represented by the touchpoint. Line numbers,
  physical line counts, function layout, and ordinary prose are supplementary
  point-in-time evidence only and MUST NOT be maintained as equality assertions
  against later source revisions. Do not copy complete functions into an ADR.
  This rule does not retroactively invalidate an unchanged Accepted record.
- Every source or configuration ADR MUST contain a Contract-To-Check
  Traceability table before acceptance. Each normative public or internal
  contract clause that states a required response, transition, ordering,
  invariant, limit, failure outcome, or prohibition MUST have a stable clause
  ID and map to at least one declared acceptance check or deterministic test.
  One check MAY cover multiple clauses only when its inputs and assertions
  exercise each clause explicitly; an uncited implication is not coverage.
- Every source or configuration ADR MUST contain a Risk Coverage Matrix with
  exactly these baseline dimensions: concurrency and ordering; timeout and
  deadline; cancellation and interruption; resource bounds and backpressure;
  and framework or trust-boundary rejection. Before acceptance, every row MUST
  identify applicability, a concrete scenario or a specific `N/A` reason, the
  owning boundary, a deterministic verification method, an exact expected
  result, and linked acceptance-check IDs. Before the implementation pull
  request is review-ready or the ADR becomes `Complete` or `Verified`, every
  applicable row MUST be `Pass` with stable evidence; `Fail` blocks both.
  The matrix supplements rather than replaces acceptance checks.
- A proposed ADR check is invalid when an independent reviewer must interpret
  what success means. Unqualified language such as "works", "normal",
  "reasonable", "appropriate", "complete", "optimized", "user-friendly",
  "stable", or "meets expectations" MUST NOT appear as the acceptance
  criterion. Such terms are allowed only when immediately anchored to an exact
  threshold, enumerated state, reproducible procedure, or authoritative
  specification section.
- An ADR MUST NOT become `Accepted` until every acceptance check is precise
  enough to be executed or deterministically inspected. `Complete` or
  `Verified` requires every applicable check to be `Pass` with actual result and
  evidence; `Fail` blocks completion, and `N/A` requires a specific reason tied
  to the check's stated trigger.
- Approval, metadata maintenance, preflight, verification, recovery, and
  archival are record gates or lifecycle stages rather than additional
  subtasks unless the record explicitly defines one of them as a separate
  deliverable.
- Before generating a new Full or Lightweight ADR, inspect only rows whose
  `Type` is Full ADR or Lightweight ADR in `docs/adr/INDEX.md`; OCR rows do not
  participate in or block ADR serialization. If any project or service ADR has
  an Implementation Status other than `Complete`, `Verified`, or `Not
  Applicable`, a new ADR MUST NOT be drafted, copied from a template, assigned
  a number or filename, or added to the index. Complete the existing ADR, or
  move it to a truthful terminal status with supporting evidence, before
  generating another ADR. This serialization gate applies repository-wide; it
  does not prohibit an OCR required to complete the current ADR.
- One exception exists when the sole non-terminal ADR is `Accepted` and
  `Blocked`, its recorded blocker cannot be resolved within that ADR's accepted
  scope or by an OCR, and resolving it requires a Full ADR concern. In that
  case, one Full ADR MAY be generated solely to remove the named blocker. The
  blocked ADR and blocker-resolution ADR MUST cite each other's exact paths,
  the new ADR MUST identify the blocker evidence and have no unrelated outcome,
  and the index MUST contain no non-terminal ADR other than those two. No
  second blocker-resolution ADR may be active; a blocker discovered in the
  exception ADR MUST be handled by updating and reapproving that exception ADR
  or by truthfully retiring one of the two records before another ADR is
  generated. Once the exception ADR is terminal, resume or truthfully retire
  the original blocked ADR.

#### Project vs. Service ADR/OCR Routing

Every decision record belongs to exactly one ADR root. Route it with this
order of precedence — stop at the first row that applies:

| Signal | Root |
| --- | --- |
| Crosses more than one service or package boundary | Project root `docs/adr/` |
| Defines or changes a contract, protocol, schema, or API consumed outside one service | Project root `docs/adr/` |
| Governs repository-wide process, security baseline, CI, or this `AGENTS.md` itself | Project root `docs/adr/` |
| Affects only one service's internal implementation, with no external contract change | That service's `<service>/docs/adr/` |
| Still ambiguous after the rows above | Project root `docs/adr/`; a later ADR may formally delegate a narrower area to a service root |

A record never sits in a service's root because it was convenient to draft
there — apply the table before creating the file.

#### Identity, Storage, Filenames, and Provenance

- **Project-wide ADRs**: `docs/adr/ADR-NNNN-<slug>.md`.
- **Project-wide OCRs**: `docs/adr/ocr/OCR-NNNN-<slug>.md`.
- **Single-service ADRs**: `<service>/docs/adr/ADR-NNNN-<slug>.md`.
- **Single-service OCRs**: `<service>/docs/adr/ocr/OCR-NNNN-<slug>.md`.
- ADRs and OCRs are never mixed in the same directory, and the `ADR-`/`OCR-`
  filename prefix must match the directory (`ocr/` holds only `OCR-*` files;
  its parent ADR root holds only `ADR-*` files). This prefix-plus-directory
  pair is how an agent tells an ADR from an OCR without opening the file.
- `NNNN` is an increasing decimal number, unique only within its own directory
  (an ADR root and its `ocr/` subfolder each keep an independent sequence).
  Pad values from 1 through 9999 with leading zeroes to four digits; after
  `9999`, continue with `10000` and higher without truncation or a new sequence.
  Before merging a new record, check that its number is unique in its directory;
  resolve a collision by renumbering the new record.
- The record's identity is its repository-relative path, never a bare
  `ADR-NNNN` or `OCR-NNNN` — the same number is reused across different roots
  and across the ADR/OCR split. Historical records are not renamed solely to
  adopt this rule.
- A governed file MUST cite the record's current repository-relative path at
  its first legal comment position when its format permits comments. Keep a
  shebang, encoding declaration, or mandatory header first when required by
  the format. When a cited record is archived (see below), update the marker
  to the new archive path, or remove it if the governed code was deleted, in
  the same change that performs the archival.
- Generated, vendored, lock, binary, and comment-free formats are tracked in
  the record's affected-files evidence rather than modified solely to add a
  marker.
- Final evidence SHOULD use a commit SHA, Git blob, PR permalink, stable
  symbol or heading, and command result. Mutable workspace line numbers are
  only supporting detail.

#### Archival

An ADR or OCR is archived — not deleted — once it stops being live guidance,
so agents can tell active decisions from historical ones without opening every
file:

- Archive an ADR once either its Decision Status is `Rejected` and its
  Implementation Status is `Not Applicable`, or its Decision Status is
  `Deprecated` or `Superseded` and its Implementation Status is `Verified`,
  `Complete`, or `Not Applicable`.
- Archive an OCR once its Decision Status is `Deprecated` or `Superseded`, or
  its Implementation Status reaches a final state (`Verified`, `Complete`,
  `Blocked` with no further attempt planned, or `Not Applicable`) — an OCR
  documents a one-time operation, so a fully executed or abandoned attempt is
  archival-eligible on its own.
- Archiving is a single change that: moves the file to `archive/` under its
  own ADR root (`docs/adr/archive/`, `docs/adr/ocr/archive/`,
  `<service>/docs/adr/archive/`, or `<service>/docs/adr/ocr/archive/`) keeping
  its filename; updates every code marker and cross-reference that cited the
  pre-archive path; and updates the record's row in `docs/adr/INDEX.md`. Only a record
  with Decision Status `Superseded` requires a counterpart record: set the new
  record's `Supersedes` and the old record's `Superseded By` to each other's
  final repository-relative path. For deprecation or terminal OCR archival
  without a replacement, retain `Superseded By: None`.
- A record is never left `Deprecated`/`Superseded` with a stale live-path code
  marker, and never physically moved without updating what pointed to it —
  the "Archival" checklist inside each template enforces this order.

#### Index

The repository maintains exactly one decision-record index at
`docs/adr/INDEX.md`. It lists every project or service ADR and OCR — active and
archived — with its type, ID, title, Decision Status, Implementation Status,
scope, Architecture Source, path, and `Superseded By`. Update the record's
single row whenever any indexed field changes, including creation, type, title,
Decision Status, Implementation Status, scope, Architecture Source, path, or
supersession metadata. OCRs use the governing ADR or `N/A — <reason>` as their
Architecture Source when no ADD task candidate applies. Rows are never deleted,
only updated, so the index remains a complete repository history. ADR/OCR
directories and number sequences remain separate; sharing an index does not
merge their identities.

#### Operational Change Records

Disposable Verification Execution does not require an OCR, even when the tool
internally compiles a transient test executable or writes isolated temporary
output. Before completion, delete that disposable output and confirm that no
reusable artifact was retained, published, promoted, loaded, deployed, or made
an input to a later operational step, and that no running, shared, or external
system was mutated. A failed test does not retroactively require an OCR; retain
its command, result, and diagnostic as normal verification evidence. If the
output or side effects exceed this boundary, stop and classify the action as a
Governed Build or other Operational work before continuing.

Every OCR MUST record its operation type, target scope, owner, approved
immutable input source or version, expected output or target state, actual
immutable artifact when the operation produces one, preflight, and one runbook
covering execution, success verification, stop condition, and recovery or
rollback. It MUST NOT include secrets or production credentials. Production,
multi-environment or phased operations, expected user/downstream/SLO impact,
or work that crosses a stated change window MUST additionally record the
window, impact, observation, and communication. For a build-only operation,
recovery is the documented artifact quarantine or discard disposition plus
confirmation that no promotion occurred. Missing a defined
recovery/disposition or stop condition upgrades the work to a Full ADR.

A Docker build OCR MUST bind the reviewed source commit to the expected image
coordinates and record the resulting immutable repository digest or, when the
image remains local and has no repository digest, the immutable image ID and
its disposition or Kubernetes load path. A Kubernetes operation OCR MUST
discover and record the actual context, namespace, workload, reviewed manifest
or configuration revision, intended image identity, previous recoverable state,
rollout and health criteria, relevant event or diagnostic result, and recovery
verification. A local Docker-hosted Kubernetes target does not waive the OCR
gate. Any disposable-sandbox exception requires a separate Accepted Full ADR.

An OCR cannot modify a Dockerfile, Makefile, CI, pipeline, artifact format,
signing, credentials, deployment topology, API/schema/protocol,
authentication, security policy, data lifecycle, dependency, provider, or
irreversible behavior. Those changes require a Full ADR even when paired with
a release operation. Every release or remote Git tag operation MUST also follow
`docs/delivery/releases.md` and `docs/delivery/git-tags.md`.

#### Approval and Status

This section is the single canonical approval contract for ADDs, ADRs, and
OCRs. Templates and instantiated documents MUST keep approval metadata but MUST
NOT define another `Approval Contract` section.

Before requesting approval, the surrounding task, review, Trello card, or pull
request context MUST unambiguously identify exactly one repository-relative ADD,
ADR, or OCR path. An eligible approver approves it by responding with the exact,
case-sensitive word `Approve`. The approver is not required to include the path,
explanation, Git commit, Git blob, content hash, revision ID, or permalink in the
approval response.

Every document MUST identify its author, decision or architecture owner,
required approver, status, scope, approver, approval time, and approval evidence.
`Approval Evidence` MUST be exactly `Approve`; no revision field is binding
approval evidence. `Approver` MUST record the concrete approving identity as
`@<actor-id>`. Examples are `@codex`, `@kimi`, or `@zcode` for known AI actors.
Approval Time, Rejection Time, and Retirement Time MUST use an ISO 8601
date-time with an explicit UTC offset (`Z` or `±HH:MM`) whenever their fields
are triggered.
For a human approver, use the authenticated account identifier supplied by the
approval system or tool. If none is available, the human MUST self-declare one
concrete `@<actor-id>` in the same approval context before responding
`Approve`. The machine login reported by `id -un` MAY be retained as
supplementary execution context but MUST NOT determine or substitute for the
human identity. The author or drafting agent MUST NOT approve that same
document. Type or role labels such as `Human`, `agent`, or `reviewer` are
invalid final `Approver` values. Do not infer or hard-code a repository account
as the human identity.

A Trello or pull-request comment containing exactly `Approve` MAY serve as
approval when its context identifies one document and its author is eligible.
A successful automatic review, card move, card closure, label, reaction, or any
word other than `Approve` is insufficient. Approval applies to the document
content present when the approval is given but is intentionally not bound to a
revision identifier.

The document MAY record an `Approval Context Revision` for audit convenience
only when that immutable revision exactly represents the document content
present for approval. The field MUST be labeled informational and non-binding;
it is not approval evidence and does not change the reapproval rules. A HEAD
revision that omits uncommitted document changes MUST NOT be recorded as the
approval context revision.
When approval occurs before the approved document is committed, this optional
field is normally added as an evidence-only update after the first immutable
revision containing that approved document content exists. Adding only this
field does not invalidate the approval.

An AI approver may approve a document only when authoritative evidence resolves
every question and consequential assumption whose answer could change
approval-invalidating content. Any remaining material uncertainty requires a
recorded determination from a specifically identified human
`@<actor-id>` established by the identity rules above before that human or
another eligible approver responds `Approve`.

A Proposed ADR or OCR may enter `Rejected` only when its named Decision Owner
or an actor authorized by its Required Approver field responds with the exact,
case-sensitive word `Reject` in a context that unambiguously identifies that
record. The actor identity follows the same authenticated-or-self-declared
rules used for an approver. The author may reject the record only when also
named as its Decision Owner; the self-approval prohibition does not prevent
rejection because rejection authorizes no implementation or operation. In the
same change, record `Rejector: @<actor-id>`, `Rejection Time`, and `Rejection
Evidence: Reject`, set Decision Status to `Rejected`, and set Implementation
Status to `Not Applicable`. No other actor or evidence may cause a rejection.

An ADD, ADR, or OCR retires only when its named Architecture/Decision Owner or
an actor authorized by its Required Approver field responds with the exact,
case-sensitive word `Deprecate` or `Supersede` in a context that unambiguously
identifies that document. `Supersede` also requires the exact replacement path;
the replacement ADR/OCR MUST be `Accepted`, and the replacement ADD MUST be
`Current`. Actor identity and author-as-owner rules are the same as for
rejection. In the same change, record `Retired By: @<actor-id>`, `Retirement
Time`, `Retirement Evidence: Deprecate` or `Supersede`, and `Retirement Reason`.
For supersession, update reciprocal `Supersedes` / `Superseded By` paths in that
same change.

An ADR or OCR MUST NOT enter `Deprecated` or `Superseded` with Implementation
Status `Not Started`, `In Progress`, or `Blocked`. The retirement change MUST
also set a truthful final Implementation Status: use `Not Applicable` when no
implementation or operation occurred, or when every partial effect was fully
reverted, quarantined, or otherwise removed with evidence; retain or set
`Complete` / `Verified` only when the implemented outcome and every applicable
check have the required evidence. This is the clean retirement path for a
permanently abandoned `Blocked` record. If partial governed behavior remains,
the record cannot be retired as `Not Applicable`; resolve it within the
Accepted scope or use the applicable blocker-resolution path before retirement.

To supersede a non-terminal ADR without violating serialization, first
`Deprecate` it and set Implementation Status to `Not Applicable` with the
required evidence, then return or defer any selected ADD candidate. Only after
that terminal state is indexed may the replacement ADR be drafted and accepted.
Finally, change the old ADR from `Deprecated` to `Superseded` and add reciprocal
replacement paths. Do not draft the replacement before the old ADR is terminal.

For this guide, a material question or uncertainty is one whose answer could
change approval-invalidating content. An **approval-invalidating change** is an
edit that changes any of the following approved content:

- For an ADD: the requirement or acceptance-outcome baseline, goals or
  non-goals, capabilities or business rules, data model or invariants,
  component responsibilities or boundaries, control or interaction flows,
  cross-cutting constraints, traceability, or ADR task candidates.
- For an ADR: the complete task outcome, scope, constraints, resolved question
  on which the decision depends, decision drivers, considered or selected
  option, rationale, consequences, primary implementation boundary, subtasks,
  affected paths, contract traceability, Risk Coverage Matrix, acceptance
  checks, or migration and rollback strategy.
- For an OCR: the complete task outcome, operation type, target or owner,
  approved input, expected output or artifact contract, scope, preflight,
  execution or verification runbook, stop condition, or recovery and rollback
  behavior.

Typographical or formatting fixes, path maintenance after a move, permitted
status transitions, approval metadata, and actual evidence or result updates
are approval-preserving only when they do not change any content listed above.
A mixed change is approval-invalidating. If classification is disputed or
remains unclear, stop before further mutation and obtain a recorded
determination from the document's named decision or architecture owner.

- Decision Status: `Proposed`, `Accepted`, `Rejected`, `Deprecated`, or
  `Superseded`.
- The complete ADR/OCR Decision Status transition set is `Proposed` to
  `Accepted` by approval, `Proposed` to `Rejected` by rejection, `Accepted` to
  `Proposed` after an approval-invalidating change, `Accepted` to `Deprecated`
  or `Superseded` by retirement, and `Deprecated` to `Superseded` when an
  accepted replacement is identified later. `Rejected` and `Superseded` are
  terminal; `Deprecated` may only become `Superseded`. A terminal record is not
  reactivated — create a new record instead. No other transition is permitted.
- Implementation Status: `Not Started`, `In Progress`, `Blocked`, `Complete`,
  `Verified`, or `Not Applicable`.
- Setting an ADR or OCR Decision Status to `Rejected` MUST set its
  Implementation Status to `Not Applicable` in the same change.
- Only an `Accepted` ADR or OCR may enter `In Progress` or `Blocked`.
- An `Accepted` ADR or OCR may enter `Blocked` only from `Not Started` or `In
  Progress`. In the same change, record its prior Implementation Status, the
  specific blocker and evidence, blocker owner, and deterministic exit or
  recheck criterion. To resume, satisfy that criterion and return to the
  recorded prior status. To abandon the work, use the retirement ritual and
  `Not Applicable` evidence defined above. It MUST NOT move directly from
  `Blocked` to `Complete` or `Verified`.
- `Verified` requires concrete completion evidence for every applicable
  checklist item.
- An approval-invalidating change to an `Accepted` ADR or OCR returns it to
  `Proposed`, resets its Implementation Status to `Not Started` in the same
  change, and requires a new `Approve`. Retain earlier implementation evidence
  as historical evidence, but do not count it toward the revised subtasks or
  acceptance checks unless those checks are executed again after reapproval.
- An approval-invalidating change to a `Current` ADD returns it to `Draft` and
  requires a new `Approve`.
- Before either reset, append the prior Approver, Approval Time, `Approval
  Evidence: Approve`, any Approval Context Revision, invalidation time, and
  invalidation reason to the Change Log. Replace the active Approver, Approval
  Time, and Approval Evidence values with `Pending — reapproval required`, and
  remove the active Approval Context Revision until a later approval records a
  new applicable value. A `Proposed` ADR/OCR or `Draft` ADD awaiting reapproval
  MUST NOT display a bare `Approval Evidence: Approve` or a revision from the
  invalidated approval in active metadata. A later approval populates the
  active fields while retaining the historical Change Log entry.

AI agents and humans with concrete `@<actor-id>` identities may draft or approve
ADDs, ADRs, and OCRs, but the author or drafting agent may not approve the same
document. Apply the approval-invalidating change rules above before
implementation or operation continues.

## Execution Workflow

1. Read this guide and every more-specific instruction that applies to the
   requested scope.
2. Inspect the relevant code, documentation, build files, and current worktree
   before proposing or making changes.
3. Classify the change. Read-only work needs no task branch. Except for the
   mandatory new-ADD branch in step 4, keep an existing authorized task branch
   or worktree. When files will change and no authorized task branch exists,
   follow the version-control policy below before drafting or submitting a
   required decision record.
4. For product demand, inspect and route the Trello requirement baseline. When
   creating a new ADD, create and switch from the current local `dev` to its new
   dedicated task branch before copying the template, assigning its number,
   creating its file, or adding its index row; never use `main` as the branch
   point. Then draft or update the ADD, complete its solution-level design and
   traceability, and make it `Current` before selecting an implementation task.
   Do not add task-level implementation design to the ADD.
5. Before drafting an ADR, enforce the repository-wide ADR serialization gate.
   Select one eligible ADD task candidate when the work derives from product
   demand, route the permitted decision record to the correct ADR root and
   directory, create the reciprocal ADD and ADR references in the same change,
   then satisfy every configured approval gate before implementation.
6. When a Trello card coordinates the work, link its stable URL from the
   record or pull request and treat its state as a projection. Do not create or
   mutate a card unless an Accepted workflow policy or the explicit bootstrap
   authorization in this guide authorizes that external change.
7. Search for existing internal capabilities and choose the smallest coherent
   change that meets the request.
8. Implement only the approved scope and preserve unrelated worktree changes.
9. Apply the automatic-review rule in Work Coordination after each push and
   before merge or operational use.
10. Run the narrowest relevant non-interactive checks, then the broader checks
    required by the affected routing rows. The installed SonarQube pre-commit
    hook scans the exact index; pre-push requires zero incremental issues, a passing analysis-bound Quality Gate, and at least
    80% coverage of changed executable lines. Routine scans, installation and
    disposable test databases through the canonical workflow require no ADR,
    OCR or repeated approval. Other external operations keep their normal gates.
    Clean disposable output and retain only safe verification evidence.
11. For a governed build, release, Git tag, or local Kubernetes action, read
    its applicable delivery or platform standards, create and accept an OCR
    before execution, then bind its actual source, artifact or tag, target,
    verification, stop, and recovery evidence.
12. Update the decision record's evidence and its row in `docs/adr/INDEX.md` whenever
    an indexed field changes. If the record is now archival-eligible, archive it
    in this same change.
13. Synchronize authoritative Git and record transitions to a linked Trello
    card when an Accepted workflow policy or the explicit bootstrap
    authorization provides that capability; never infer a repository
    transition from card state.
14. Report changed files, verification results, known limitations, and stable
    evidence. Do not claim completion while required work remains.

## Scope Routing

For each affected path or operation, evaluate the rows below in listed order
and stop at the first matching row. A change with multiple affected paths or
operations applies the independently matched row for each one.

| Scope | Read first | Working directory | Verification command | Notes |
| --- | --- | --- | --- | --- |
| `AGENTS.md`, `AGENTS.template.md`, `CLAUDE.md` | This guide's Non-Negotiable Gates, Execution Workflow, and Version-Control Safety sections | repository root | `npm test --prefix tools/governance-validator`; `npm run validate --prefix tools/governance-validator` | Run deterministic governance validation and perform a structured review of the affected instruction contracts. Documentation-only changes do not require Red-Green-Refactor. |
| `docs/architecture/**` or `<service-or-package>/docs/architecture/**` | `docs/README.md` and this guide's Document Requirement Levels and Architecture Design Documents sections | repository root | `npm test --prefix tools/governance-validator`; `npm run validate --prefix tools/governance-validator` | Validate requirement levels, template fields, status, index and reciprocal links, and Mermaid syntax/ID coverage; also review Trello baseline capture, Figma references, solution completeness, task-detail boundary, and traceability. |
| `docs/**` | `docs/README.md` and this guide's Document Requirement Levels, Architecture Design Documents, and Decision Records sections | repository root | `npm test --prefix tools/governance-validator`; `npm run validate --prefix tools/governance-validator` | Validate requirement levels, template fields, lifecycle status, index rows, paths, and cross-references, then perform the applicable structured review. Documentation-only changes do not require Red-Green-Refactor. |
| `.githooks/**`, `scripts/sonar-quality-gate.sh`, `tools/sonarqube/**` | `docs/README.md`, common engineering and Python standards, `tools/sonarqube/README.md` | repository root | `python3 -m unittest discover -s tools/sonarqube -p 'test_*.py'`; `ruff check tools/sonarqube`; `ruff format --check tools/sonarqube`; `python3 tools/sonarqube/gate.py check --revision HEAD` | Canonical automatic commit/push gate; owner-authorized routine operation without ADR/OCR. Sonar scans run locally; CI retains hook regression and ordinary project checks. |
| `tools/governance-validator/**` | `docs/README.md`, `docs/development/software-engineering-standard.md`, and this guide's Document Requirement Levels and Decision Records sections | `tools/governance-validator` | `npm test`; `npm run validate`; from repository root `python3 tools/sonarqube/gate.py check --revision HEAD` | This validator and its tests are source work: develop behavior test-first and keep dependencies exactly locked. |
| `.github/workflows/koduck-ai.yml` | `docs/README.md`, `docs/development/software-engineering-standard.md`, `docs/adr/ADR-0002-required-ai-ci-postgres-verification.md`, and this guide's Work Coordination and Decision Records sections | repository root | `npm test --prefix tools/governance-validator`; `npm run validate --prefix tools/governance-validator` | Keep every routed governance command inside an existing required `dev` check and preserve the exact three required check contexts. Configuration changes use governance validation plus a structured review of the routed commands and required check contexts. |
| `koduck-ai/**`, root `Cargo.toml`, or root `Cargo.lock` | `docs/README.md`, `docs/development/software-engineering-standard.md`, and `docs/development/rust-standard.md` | repository root | `cargo fmt --all --check`; `cargo clippy -p koduck-ai --all-targets --all-features -- -D warnings`; `cargo test -p koduck-ai --all-targets --all-features`; `python3 tools/sonarqube/gate.py check --revision HEAD` | Use non-interactive commands. The canonical SonarQube workflow needs no ADR/OCR. These commands need no OCR when they satisfy Disposable Verification Execution; a retained, published, promoted, loaded, deployed, or later-consumed artifact is a Governed Build and requires an Accepted OCR. |
| Release or Git tag operation | `docs/delivery/releases.md`, `docs/delivery/git-tags.md`, and the governing Accepted OCR | repository root | Commands approved by the OCR | Treat tag creation or mutation, release publication, and artifact publication as external operational writes. |

Verification commands in a source or configuration routing row need no OCR
when they satisfy Disposable Verification Execution. A routing row MUST NOT
classify incidental verification compilation as a Governed Build; retained,
published, promoted, loaded, deployed, or later-consumed artifacts remain
Governed Builds and require an Accepted OCR.

If no Scope Routing row matches, discover and run the narrowest relevant
non-interactive check for every affected path. If no automated check exists,
perform a structured review, report that no configured automated check was
available, and record what was inspected instead of inventing a command.

Before adding the first maintained source or configuration path for a new
service, package, language, or platform, add its explicit Scope Routing row in
the same governed change, including applicable standards, working directory,
and non-interactive verification command. The fallback above supports discovery
and exceptional unmatched paths; it MUST NOT become the permanent route for a
maintained source or configuration area.

## Security

- Validate external input at trust boundaries.
- Apply least privilege to users, services, credentials, and infrastructure.
- Use approved libraries for cryptography, authentication, and authorization.
- Avoid unsafe command construction, raw query concatenation, and untrusted
  deserialization.

## Version-Control Safety

- Inspect the worktree before changing branches, pulling, staging, or
  committing.
- Stage explicit approved paths and inspect the staged diff before committing.
- Do not run destructive or externally publishing operations without the
  authorization required by the project.

### Branch, Pull Request, And Commit Policy

The current non-protected branch or worktree present when a user starts a task
is authorized for that task unless the user says otherwise. A different or new
branch is authorized only when the user explicitly selects or requests it, an
Accepted record or workflow names it, or an already-authorized task tool creates
and assigns it. A branch name alone never authorizes task scope or external
writes.

- **Protected branches**: `main` and `dev` are protected branches; no direct
  commits or pushes to either branch.
- **Task-branch base**: `dev` is the sole permitted base for every new task
  branch. Create from the current local `dev`; `main` MUST NOT be used as a
  branch point. Do not fetch, pull, or otherwise synchronize `dev` unless the
  user authorizes it.
- **Task branches**: Except for the mandatory new-ADD branch below, keep an
  existing authorized task branch or worktree rather than switching solely to
  satisfy naming. When a new branch is needed, create it from the current local
  `dev` after inspecting the worktree, using a project-appropriate `feature/`,
  `fix/`, `docs/`, or `chore/` prefix or a tool-mandated prefix. Do not fetch,
  pull, or otherwise synchronize a branch unless the user authorizes it.
- **New ADD branches**: A task that creates a new ADD from identified Trello
  requirements MUST use a newly created task branch dedicated to that ADD. This
  workflow rule authorizes creating and switching to that branch; it does not
  authorize Trello mutation, remote synchronization, pushing, or other external
  writes. Create the branch from the current local `dev` before the first ADD
  mutation. Updating an existing ADD follows the normal task-branch rule.
- **Task pull requests**: target `dev`; include the verification commands run and
  their results, link the governing decision record when one applies, and link
  the coordinating Trello card when one exists. Apply the automatic-review rule
  and the original-thread reply rule in Work Coordination; passing review does
  not replace record acceptance or operational verification. Only a
  repository-integration or release pull request from `dev` may target `main`;
  ordinary task branches MUST NOT target `main`.
- **Commits**: use Conventional Commits (`<type>(<scope>): <imperative
  summary>`) with an appropriate type such as `feat`, `fix`, `docs`,
  `refactor`, `test`, `chore`, `ci`, or `build`. Every commit message MUST
  include separate `ADR:`, `OCR:`, and `Trello:` footer lines. An applicable
  ADR or OCR line MUST list one or more exact repository-relative record paths;
  an applicable Trello line MUST list one or more stable card URLs. Separate
  multiple references with `,`. When a reference type does not apply, write
  `N/A — <reason>` on that line. Do not omit a line, leave a placeholder, or
  create a record or card solely to populate the commit message. Automatically
  generated merge commits whose message cannot be customized are exempt; the
  merged pull-request body MUST carry the three references instead. Squash,
  rebase, and manually authored merge commits whose messages can be edited are
  not exempt.

This policy uses protected `dev` as the task pull-request target and sole
task-branch base, protected `main` as the integration or release target, and
temporary task branches. Revisit it with a Full ADR once
additional long-lived release branches, independent component versioning, or
multi-environment promotion is needed. Release and Git tag operations follow
`docs/delivery/releases.md` and `docs/delivery/git-tags.md`.

## Verification And Completion Evidence

- For a source-code feature or reproducible defect fix, use Red-Green-Refactor:
  first add the smallest focused test, observe it fail for the expected missing
  behavior, then implement and observe focused and routed checks pass. Pure
  documentation changes do not manufacture a failing source test; they run the
  existing governance validator and structured review instead.
- Run focused checks first and broader checks when shared behavior is
  affected.
- Use the exact commands and working directories from Scope Routing.
- Report skipped, blocked, or failing checks with their full reason.
- Prefer stable evidence such as commit identifiers, immutable links, symbols,
  headings, and command-result summaries. Treat mutable line numbers as
  supplementary evidence only.
- ADR source evidence SHOULD cite fully qualified symbols or stable contract
  anchors and MAY include a short decisive code excerpt when the symbol alone
  is insufficient. It MUST NOT require current source to preserve an exact
  physical line count, function arrangement, or ordinary wording.
- Completion requires the requested behavior, documentation, required checks,
  and stable evidence. For the SonarQube workflow, the exact source tree and
  baseline must have zero incremental unresolved issues,
  a passing analysis-bound Quality Gate, and at least 80% coverage of changed
  executable lines. A proven zero executable-line diff is permitted; missing
  coverage is a failure. The hook workflow defines the Git-based increment and
  same-source coverage proof in `tools/sonarqube/README.md`.
- ADR verification MUST be reproducible from each declared acceptance check's
  preconditions, method, and expected result. Evidence without a predetermined,
  binary acceptance point does not prove completion.
- Source and configuration verification MUST include the governing ADR's
  complete Contract-To-Check Traceability table and Risk Coverage Matrix. A
  green broad test suite does not compensate for an unmapped contract clause,
  an unassessed baseline risk dimension, or a failing matrix row.
- Document review MUST confirm that every `[Required]` item is complete, every
  conditional trigger is assessed and satisfied or explicitly not applicable,
  and every retained `[Optional]` item is accurate and complete. Run
  `npm run validate --prefix tools/governance-validator` for every affected
  governance document; automated validation supplements rather than replaces
  the structured semantic review.

### Local SonarQube Feature Completion Gate

The repository owner's direct instruction on 2026-09-05, recorded in
`tools/sonarqube/README.md`, enables the canonical workflow without a new ADR.
For this workflow it overrides ADR-0015's conditional routing activation and
per-operation approval requirements. Historical accepted records remain
historical evidence, not approval of this later instruction.

- Install with `sh tools/sonarqube/install.sh`. Every commit scans the effective
  index through `.githooks/pre-commit`; every push checks its actual proposed
  ref targets through `.githooks/pre-push`. Routine installation, disposable
  test databases and analysis require no ADR, OCR or repeated `Approve`.
- The only scanner entry point is `python3 tools/sonarqube/gate.py` with
  `pre-commit`, `pre-push`, or `check --revision <commit>`; configuration,
  timeouts, stable scope and coverage tools are pinned under `tools/sonarqube/`.
  Do not improvise scanner parameters, weaken a finding, or claim a scan from
  an unrelated revision as passing evidence.
- A completed analysis may be committed locally with findings for repair.
  Push, feature completion and review-ready status require zero incremental
  unresolved issues, Quality Gate `OK` for the exact
  analysis, and at least 80% coverage of the feature's changed executable lines.
  Analysis failures block commits; missing/stale evidence blocks pushes.
- Pre-commit scans a disposable Git snapshot of the effective index and records
  its tree identity; pre-push matches the proposed commit's tree. Unstaged work
  is excluded. Coverage is generated/imported from that same snapshot. The
  index is rechecked after scanning. Source, baseline or policy changes
  invalidate evidence.
- The baseline is local `dev`'s merge base with the target; manual checks may
  specify an ancestor with `--base`. Identically scoped base and target analyses
  establish incremental
  finding counts. Imported coverage intersected with the Git diff establishes
  changed-line coverage. The server's rolling New Code period is not presented
  as an exact feature diff. Missing report data cannot pass as zero new lines.
- Host-local scans are serialized. The existing token uses project-level issue
  and file-metric reads, as explicitly selected by the owner; these cannot prove
  isolation from concurrent scans on another host. Every push scans afresh.
  Hotspot review is not independently checked with this token. Failed
  candidate analyses remain visible; never rescan old code to hide a failure.
- Use `KODUCK_SONAR_TOKEN` only from the existing shell environment. Never
  print it or place it in arguments, repository files,
  logs or evidence. Only the scanner and API client receive the analysis token;
  test subprocesses do not inherit it. Do not change project permissions,
  profiles, gates, finding state or credentials as a routine scan.
- Record tree/revision, base, policy hash, compute task and analysis IDs,
  incremental issue counts, Quality Gate and coverage numerator and
  denominator under the Git common directory. Generate fresh evidence before push.
  Per the owner’s 2026-09-06 instruction, CI does not run Sonar or build a
  runner image. Local hooks enforce Sonar admission; CI does not prove Sonar
  compliance when hooks are bypassed.

## Sources Of Truth

| Concern | Authoritative path or discovery command |
| --- | --- |
| Architecture solution views and future ADR task candidates | `docs/architecture/INDEX.md` and the indexed ADD paths |
| Architecture design template | `docs/architecture/template/0000-add-template.md` |
| All project and service ADR/OCR identities and current statuses | `docs/adr/INDEX.md` |
| Decision-record templates | `docs/adr/template/` |
| Documentation navigation and standards catalog | `docs/README.md` |
| Release eligibility, publication, evidence, and recovery | `docs/delivery/releases.md` |
| Git tag format, creation, verification, and immutability | `docs/delivery/git-tags.md` |
| Web and native-client visual and interaction design | Specific Figma file or node identified by the task, ADR, or PR |
| Demand, ownership, priority, progress, blockers, and discussion | [Koduck Trello board](https://trello.com/b/Kz9qnd3D/koduck), as a mutable coordination view only |
| Versioned source, configuration, automatic-review scope, and implementation evidence | Git commit and pull-request revisions |
| Built container artifact identity | Docker repository digest, or documented immutable local image ID and cluster load path |
| Observed local runtime state | Kubernetes context, namespace, workload status, and diagnostics recorded by the governing OCR |
| Project structure, dependencies, and executable commands | Root and service manifests/build files, including `Cargo.toml` and `koduck-ai/Cargo.toml` |
