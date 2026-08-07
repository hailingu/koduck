# Koduck Agent Guide

> Language: English

Koduck is a from-scratch rebuild of `koduck-quant`, restructured to fix
governance and repository-organization problems found during that project's
development — most importantly, ambiguous ADR/OCR naming, missing ADR
archival, and unclear project-level vs. service-level decision-record scope.
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

## Non-Negotiable Gates

### Core Rules

- Preserve user and other-task changes. Do not overwrite, reformat, stage, or
  clean unrelated work.
- Never hard-code, expose, or log secrets, tokens, passwords, private keys, or
  sensitive user data.
- Any implementation requiring a decision record MUST wait until that record is
  explicitly `Accepted`. AI agents or humans with a concrete `@<actor-id>` may
  approve a record, but the author or drafting agent MUST NOT approve that same
  record. Material uncertainty requires a recorded determination from a
  specifically identified human before acceptance.
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
blockers, and discussion. A card MAY link to an ADR, OCR, branch, commit, or
pull request, but the card body, comments, labels, list position, closure, and
deletion do not accept, verify, deprecate, supersede, archive, or delete a
repository record.

Repository ADRs and OCRs remain authoritative for decisions, operational
authorization, approval metadata, stable evidence, and archival. Git commits
and pull requests are authoritative for versioned source and configuration,
automatic-review scope, implementation review, and immutable revision identity.
Docker owns the built image artifact identified by repository digest or, for a
local-only image, an immutable image ID with its cluster load path. The local
Docker-hosted Kubernetes cluster owns observed runtime state. An OCR binds the
reviewed Git revision and Docker image to a discovered Kubernetes context,
namespace, target state, verification result, and recovery path.

Automatic review after `git push` is a revision-bound review gate, not formal
ADR/OCR acceptance and not proof of build or deployment. A new push requires
review coverage for the new revision. A review or comment counts as approval
only when an eligible actor other than the author or drafting agent explicitly
approves the record's repository-relative path and exact Git blob or commit,
and the record copies that evidence into its approval metadata.

When Trello is used, synchronize authoritative Git and ADR/OCR transitions
outward to Trello. Automation MUST be idempotent and MUST NOT advance Git or a
record because a card was moved, closed, relabeled, or commented on. On
conflict, repository state wins; report the drift and restore or flag the card
without erasing human discussion. Never copy secrets, kubeconfig content,
private endpoints, credentials, or sensitive runtime data into Trello or
review output.

### Change Classification

Classify requested work before editing:

| Class | Includes |
| --- | --- |
| Read-only | Investigation, search, review, Q&A — no writes |
| Editorial Documentation | Markdown with no governance, contract, security, deployment, or process meaning |
| Normative / Governance Documentation | Markdown or instructions that define governance, contracts, security, deployment, or process behavior |
| Coordination Metadata | Routine Trello card creation, assignment, linking, labeling, commenting, or state synchronization under an Accepted workflow policy |
| Source | Application, library, or test code |
| Configuration | Build, CI, deployment, environment, or infrastructure config |
| Operational | A reversible build/release/deploy/rollback/runbook action against a running or artifact-producing system |

Read-only work needs no decision record. Normative/governance documentation,
source, configuration, and operational work are gated by the Decision Records
policy below. Editorial documentation is eligible for No record only when its
diff proves it has no normative meaning. Routine Coordination Metadata needs no
per-action ADR or OCR, but a change to board structure, field or label
semantics, authority mappings, connector permissions, automation behavior, or
failure handling is repository governance and requires a project-level Full
ADR.

### Decision Records

#### Classification

| Class | Use when | Gate |
| --- | --- | --- |
| Full ADR | Architecture, governance, cross-service behavior, public API/schema/protocol, security, data, dependency, or build/release/deployment strategy, pipeline, configuration, service-boundary, or irreversible decisions | Accepted before implementation |
| Lightweight ADR | A localized, reversible source behavior change whose checklist proves no Full ADR concern applies | Accepted before implementation |
| Operational Change Record (OCR) | A reversible build, release, deployment, rollback, or existing runbook operation within an accepted architecture, pipeline, artifact contract, and security/data boundary | Accepted before the operation |
| No record | Read-only work, non-normative editorial documentation, routine coordination metadata under an Accepted policy, or a provably semantics-neutral formatting/comment-only edit | No decision-record gate; normal authorization and verification still apply |

Normative Markdown about governance, contracts, security, deployment, or
process is not editorial. A formatting/comment-only edit qualifies only when
its diff proves there is no semantic change. Tests, snapshots, generated
artifacts, lockfiles, dependencies, config, and mixed-scope changes are not
automatically exempt: classify them by the behavior or decision they support,
using the highest applicable class.

Use `docs/adr/template/0000-template.md` for Full ADRs,
`docs/adr/template/0000-lightweight-template.md` for eligible Lightweight
ADRs, and `docs/adr/template/0000-operational-change-template.md` for eligible
OCRs.

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
- `NNNN` is a four-digit number, increasing and unique only within its own
  directory (an ADR root and its `ocr/` subfolder each keep an independent
  sequence). Before merging a new record, check that its prefix is unique in
  its directory; resolve a collision by renumbering the new record.
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

- Archive an ADR once its Decision Status is `Deprecated` or `Superseded` and
  its Implementation Status is `Verified`, `Complete`, or `Not Applicable`.
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
scope, path, and `Superseded By`. Update the record's single row whenever any
indexed field changes, including creation, type, title, Decision Status,
Implementation Status, scope, path, or supersession metadata. Rows are never
deleted, only updated, so the index remains a complete repository history.
ADR/OCR directories and number sequences remain separate; sharing an index
does not merge their identities.

#### Operational Change Records

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

This section is the single canonical statement of the approval contract.
Templates and instantiated records MUST keep approval metadata but MUST NOT
define an `Approval Contract` section.

Every record MUST identify its author, decision owner, required approver,
decision status, implementation status, scope, related issue or PR, approval
evidence, approval time, and approved revision. AI agents or humans may formally
approve a record when they satisfy the configured approval gate. `Approver` MUST
record the concrete approving identity as `@<actor-id>`. Examples are `@codex`,
`@kimi`, or `@zcode` for known AI actors. For a human approver, run `id -un` on
the execution machine and record the result as `@<local-login-user>`. Approval
evidence MUST identify the same actor and bind the approval to a commit, durable
content hash, or other retrievable immutable revision. The author or drafting
agent MUST NOT approve that same record. Type or role labels such as `Human`,
`agent`, or `reviewer` are invalid final `Approver` values. Do not hard-code a
repository account as the human identity.

A Trello or pull-request comment MAY serve as approval evidence only when its
permalink and content identify the same concrete approver, the record's
repository-relative path, and the exact approved Git blob or commit. A generic
approval, successful automatic review, card move, card closure, or passing
reaction is insufficient.

An AI approver may accept a record only when authoritative evidence resolves
every question and consequential assumption that could materially change the
decision. Any remaining material uncertainty requires a recorded determination
from a specifically identified human `@<local-login-user>` bound to the
approved revision before the record may become `Accepted`.

- Decision Status: `Proposed`, `Accepted`, `Rejected`, `Deprecated`, or
  `Superseded`.
- Implementation Status: `Not Started`, `In Progress`, `Blocked`, `Complete`,
  `Verified`, or `Not Applicable`.
- Only `Accepted` decisions may enter `In Progress`.
- `Verified` requires concrete completion evidence for every applicable
  checklist item.
- A substantive change to an Accepted decision, scope, constraint, option, or
  completion condition returns it to `Proposed` and requires renewed approval.
- Evidence-only updates and approval metadata do not invalidate the approved
  revision when they do not change its decision semantics.

AI agents and humans with concrete `@<actor-id>` identities may draft or approve
decision records, but the author or drafting agent may not approve the same
record. If an approved decision changes materially, apply the re-approval rule
above before implementation continues.

## Execution Workflow

1. Read this guide and every more-specific instruction that applies to the
   requested scope.
2. Inspect the relevant code, documentation, build files, and current worktree
   before proposing or making changes.
3. Classify the change. Read-only work needs no task branch. Keep an existing
   authorized task branch or worktree. When files will change and no authorized
   task branch exists, follow the version-control policy below before drafting
   or submitting a required decision record.
4. Route any required decision record to the correct ADR root (project vs.
   service) and correct directory (ADR vs. OCR), draft it there, then satisfy
   every configured approval gate before implementation.
5. When a Trello card coordinates the work, link its stable URL from the
   record or pull request and treat its state as a projection. Do not create or
   mutate a card unless the task or Accepted workflow authorizes that external
   change.
6. Search for existing internal capabilities and choose the smallest coherent
   change that meets the request.
7. Implement only the approved scope and preserve unrelated worktree changes.
8. Treat automatic review as covering only the pushed revision. Resolve
   findings and obtain fresh review coverage after a new push before merge or
   operational use.
9. Run the narrowest relevant non-interactive checks, then the broader checks
   required by the affected routing rows.
10. For a governed build, release, Git tag, or local Kubernetes action, read
    its applicable delivery or platform standards, create and accept an OCR
    before execution, then bind its actual source, artifact or tag, target,
    verification, stop, and recovery evidence.
11. Update the decision record's evidence and its row in `docs/adr/INDEX.md` whenever
   an indexed field changes. If the record is now archival-eligible, archive it
   in this same change.
12. Synchronize authoritative Git and record transitions to a linked Trello
    card when the authorized workflow provides that capability; never infer a
    repository transition from card state.
13. Report changed files, verification results, known limitations, and stable
   evidence. Do not claim completion while required work remains.

## Scope Routing

| Scope | Read first | Working directory | Verification command | Notes |
| --- | --- | --- | --- | --- |
| `AGENTS.md`, `AGENTS.template.md`, `CLAUDE.md` | This guide's Non-Negotiable Gates, Execution Workflow, and Version-Control Safety sections | repository root | None | Perform a structured review of the affected instructions and report the inspected contracts; no automated governance check is configured. |
| `docs/**` | `docs/README.md` and this guide's Decision Records section | repository root | None | Perform a structured review of affected navigation, templates, records, index rows, paths, and cross-references; no automated governance check is configured. |
| Release or Git tag operation | `docs/delivery/releases.md`, `docs/delivery/git-tags.md`, and the governing Accepted OCR | repository root | Commands approved by the OCR | Treat tag creation or mutation, release publication, and artifact publication as external operational writes. |

If no Scope Routing row matches, discover and run the narrowest relevant
non-interactive check for every affected path. If no automated check exists,
perform a structured review, report that no configured automated check was
available, and record what was inspected instead of inventing a command.

## Global Engineering Rules

- Read existing interfaces and nearby patterns before designing a change.
- Prefer an existing component, utility, client, schema, validator, hook, or
  service when it already satisfies the requirement.
- Keep the diff focused; avoid unrelated refactors, dependency additions, and
  drive-by formatting.
- Add intent-bearing documentation for new public behavior when the
  applicable standards require it.
- Use explicit error handling and structured diagnostics supported by the
  project; do not conceal failures.
- Do not hard-code environment-specific values or modify generated and vendor
  content unless the configured workflow explicitly requires it.

### Security

- Never hard-code or expose secrets, passwords, keys, tokens, or sensitive
  data.
- Validate external input at trust boundaries.
- Apply least privilege to users, services, credentials, and infrastructure.
- Use approved libraries for cryptography, authentication, and authorization.
- Avoid unsafe command construction, raw query concatenation, and untrusted
  deserialization.

### Version-Control Safety

- Inspect the worktree before changing branches, pulling, staging, or
  committing.
- Do not discard, overwrite, format, or stage unrelated user-owned changes.
- Stage explicit approved paths and inspect the staged diff before committing.
- Do not run destructive or externally publishing operations without the
  authorization required by the project.

### Branch, Pull Request, And Commit Policy

- **Protected branches**: `main` is the protected branch; no direct commits or
  pushes to `main`.
- **Task branches**: Keep an existing authorized task branch or worktree rather
  than switching solely to satisfy naming. When a new branch is needed, create
  it from the current local `main` after inspecting the worktree, using a
  project-appropriate `feature/`, `fix/`, `docs/`, or `chore/` prefix or a
  tool-mandated prefix. Do not fetch, pull, or otherwise synchronize a branch
  unless the user authorizes it.
- **Pull requests**: target `main`; include the verification commands run and
  their results, link the governing decision record when one applies, and link
  the coordinating Trello card when one exists. A pushed revision must have
  automatic-review coverage for that revision; passing review does not replace
  record acceptance or operational verification.
- **Commits**: use Conventional Commits (`<type>(<scope>): <imperative
  summary>`) with an appropriate type such as `feat`, `fix`, `docs`,
  `refactor`, `test`, `chore`, `ci`, or `build`. Every commit message MUST
  include separate `ADR:`, `OCR:`, and `Trello:` footer lines. An applicable
  ADR or OCR line MUST list one or more exact repository-relative record paths;
  an applicable Trello line MUST list one or more stable card URLs. Separate
  multiple references with `, `. When a reference type does not apply, write
  `N/A — <reason>` on that line. Do not omit a line, leave a placeholder, or
  create a record or card solely to populate the commit message.

This is a starting policy for a single-branch repository; revisit it with a
Full ADR once release branching, independent component versioning, or
multi-environment promotion is needed. Release and Git tag operations follow
`docs/delivery/releases.md` and `docs/delivery/git-tags.md`.

## Verification And Completion Evidence

- Reproduce a reported failure before fixing it when feasible.
- Run focused checks first and broader checks when shared behavior is
  affected.
- Use the exact commands and working directories from Scope Routing.
- Report skipped, blocked, or failing checks with their full reason.
- Prefer stable evidence such as commit identifiers, immutable links, symbols,
  headings, and command-result summaries. Treat mutable line numbers as
  supplementary evidence only.
- Completion requires the requested behavior, required documentation, required
  checks, and required evidence — not merely an implementation attempt.

## Sources Of Truth

| Concern | Authoritative path or discovery command |
| --- | --- |
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
| Project structure, dependencies, and executable commands | Discover from root and service manifests/build files as they are added; no service exists yet in this repository |

## Developer Principles

1. Take pride in careful research; reject guessing interfaces in the dark.
2. Take pride in seeking confirmation; reject vague execution.
3. Take pride in stating assumptions; reject hiding assumptions.
4. Take pride in reading the code first; reject skipping the reading.
5. Take pride in following existing patterns; reject ignoring established conventions.
6. Take pride in verifying against source code; reject inventing knowledge.
7. Take pride in minimal implementation; reject overengineering.
8. Take pride in implementing functionality directly; reject premature abstraction.
9. Take pride in explaining trade-offs; reject ignoring them.
10. Take pride in recommending the best option; reject listing options without judgment.
11. Take pride in minimal changes; reject broadening the diff.
12. Take pride in focusing on what is required; reject unrelated side changes.
13. Take pride in matching the existing style; reject imposing personal style preferences.
14. Take pride in preserving the original shape; reject casual drive-by formatting.
15. Take pride in reading the full error; reject blind fixes.
16. Take pride in reproducing issues first; reject fixing without reproduction.
17. Take pride in changing one thing and testing one thing; reject mixing many changes and tests.
18. Take pride in proactive testing; reject skipping verification.
19. Take pride in tracing the root cause; reject working around it.
20. Take pride in adding dependencies cautiously; reject adding packages lightly.
21. Take pride in explaining cause and effect; reject posting code without context.
22. Take pride in proactively warning about risks; reject hiding them.
23. Take pride in marking decisions clearly; reject making decisions silently.
24. Take pride in seeking approval first; reject uncontrolled refactoring.
25. Take pride in memorable naming; reject bland, forgettable names.
26. Take pride in short functions; reject long, exhausting ones.
27. Take pride in paying down debt gradually; reject letting decay accumulate.
28. Take pride in consistent style; reject arbitrary coding.
29. Take pride in shipping behind switches; reject confident bare changes.
30. Take pride in easy-to-use interfaces; reject complex ambiguity.
31. Take pride in branch warnings; reject silent neglect.
32. Take pride in monitoring and tuning; reject abandoning maintenance.
33. Take pride in using first-principles thinking to deeply analyze the essence of problems; reject focusing only on surface appearances.
34. Take pride in configuration, reject hardcoding.
35. Take pride in defining variables, reject writing magic values.
36. Take pride in solving problems; reject relying on fallbacks.
