# Koduck Documentation

This is the single navigation entry point for documentation under `docs/`.
The root [`AGENTS.md`](../AGENTS.md) remains authoritative for governance,
decision-record gates, approval, evidence, version control, and delivery rules.
Do not duplicate those rules in directory-local README files.

## Architecture Design Documents

- [`architecture/INDEX.md`](architecture/INDEX.md) is the single
  repository-wide index for every active and archived Architecture Design
  Document (ADD), including service- or package-internal documents.
- Repository-wide and cross-service ADDs live in `docs/architecture/`.
- A single service or package's internal ADDs live in
  `<service-or-package>/docs/architecture/` and retain one catalog row in the
  central index.
- ADDs translate linked Trello requirements into an overall solution view:
  functional capabilities, data models, architecture, control flows,
  interaction flows, cross-cutting concerns, traceability, and future ADR task
  candidates.
- ADDs do not authorize implementation and do not contain task-level
  implementation design. A product-development ADR selects exactly one task
  candidate from a `Current` ADD and owns detailed design, implementation, and
  verification.
- The handoff is bidirectional: the selected ADD candidate records the exact
  ADR path, and the ADR's `Architecture Source` records the exact ADD path and
  candidate ID. Both references are created and maintained in the same change.
- Archived ADDs move to the matching architecture root's `archive/` directory;
  their index rows are updated rather than deleted.

Template:

- [Architecture Design Document](architecture/template/0000-template.md)

Read the Architecture Design Documents section of [`AGENTS.md`](../AGENTS.md)
before drafting, reviewing, routing, indexing, superseding, or archiving an ADD.

## Document Requirement Levels

ADD, ADR, and OCR templates label their sections as `[Required]`,
`[Conditionally Required — <trigger>]`, or `[Optional]`. Required content must
always be completed; conditional content becomes mandatory when its stated
trigger applies and otherwise records `N/A — <reason>`; optional sections may
be removed. The canonical rules and lifecycle gates are in [`AGENTS.md`](../AGENTS.md).

## ADD, ADR, And OCR Approval, Rejection, And Retirement

When the approval context unambiguously identifies one document, an eligible
non-author approver responds with exactly `Approve`. No commit ID, Git blob,
content hash, revision ID, path, or explanation is required in the approval
response. The document records the approver identity, approval time, and
`Approval Evidence: Approve`; approval-invalidating changes, as enumerated in
`AGENTS.md`, require another `Approve`. Human identity comes from an
authenticated account or a prior self-declaration in the same context, never
from the execution machine's login. A revision may be recorded only as
informational, non-binding context when it exactly represents the approved
document. When approval is invalidated, its prior metadata and any Approval
Context Revision move to the Change Log; active Approver, Approval Time, and
Approval Evidence become `Pending — reapproval required`, and the active
Approval Context Revision is removed until a later approval records a new one.

A Proposed ADR or OCR is rejected only when its Decision Owner or an actor
authorized by its Required Approver responds with exactly `Reject`; the record
then captures the rejector and time and atomically becomes `Rejected` / `Not
Applicable`. The canonical authority and identity rules are in `AGENTS.md`.

Deprecation or supersession requires exactly `Deprecate` or `Supersede` from
the document owner or an actor authorized by Required Approver, plus retirement
identity, time, reason, and evidence. A retired ADR or OCR must have a truthful
final Implementation Status in the same change. An ADD cannot retire until all
of its candidates are `Deferred` or `Complete` and no linked ADR remains
non-terminal.

## Decision Records

- [`adr/INDEX.md`](adr/INDEX.md) is the single repository-wide index for every
  active and archived project or service ADR and OCR.
- Full and Lightweight project ADRs live directly in `docs/adr/`.
- Project OCRs live in `docs/adr/ocr/`.
- Future service records retain their service-local ADR/OCR paths but add their
  single catalog row to `docs/adr/INDEX.md`.
- ADR and OCR prefixes, directories, and number sequences remain distinct even
  though their status rows share one index.
- Archived records move into the matching `archive/` directory; the index row
  is updated rather than deleted.

Templates:

- [Full ADR](adr/template/0000-template.md)
- [Lightweight ADR](adr/template/0000-lightweight-template.md)
- [OCR](adr/template/0000-operational-change-template.md)

Read the Decision Records section of [`AGENTS.md`](../AGENTS.md) before drafting,
approving, implementing, operating, or archiving a record. Product-development
ADRs must also follow the ADD-to-ADR handoff rules above.

ADR acceptance checks must be predetermined and binary: each check identifies
its subtask, preconditions or input, verification method, exact observable
expected result, and evidence. Subjective wording without a threshold, exact
state, reproducible procedure, or authoritative specification reference is not
a valid ADR check.

## Development Standards

Before writing, modifying, or reviewing source code or infrastructure
configuration, read this catalog and every matching language or platform file
in full. When work spans multiple languages or platforms, read every applicable
file.

| File | Language / platform |
| --- | --- |
| [development/rust.md](development/rust.md) | Rust |
| [development/swift.md](development/swift.md) | Swift |
| [development/python.md](development/python.md) | Python |
| [development/typescript.md](development/typescript.md) | TypeScript |
| [development/java.md](development/java.md) | Java |
| [development/kubernetes.md](development/kubernetes.md) | Kubernetes manifests and operations |

Each standard applies to every current or future service, package, or script
using that language or platform. Inspect the affected module for established
local conventions and use its configured formatter, linter, and non-interactive
checks. A local convention may preserve consistency within its module unless it
conflicts with an Accepted decision or another binding requirement.

## Delivery Standards

Before planning or performing a release or Git tag operation, read both files
in full:

| File | Scope |
| --- | --- |
| [delivery/releases.md](delivery/releases.md) | Release eligibility, versioning, publication, evidence, and recovery |
| [delivery/git-tags.md](delivery/git-tags.md) | Release-tag naming, creation, verification, and immutability |

These standards govern repository releases and tags. They do not replace the
ADR/OCR classification and operational authorization rules in
[`AGENTS.md`](../AGENTS.md).

### Reference Freshness

Each standard records a `Last reviewed` date. It may be used without
revalidation when that date is within 180 days and its authoritative source has
not had a breaking revision. Otherwise, revalidate the source before relying on
it for a non-trivial change and update `Last reviewed`. When offline, use the
locked local content and report the limitation instead of inventing guidance.
