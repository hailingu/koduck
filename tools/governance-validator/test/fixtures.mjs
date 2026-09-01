// ADR: docs/adr/ADR-0002-required-ai-ci-postgres-verification.md

import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const validator = fileURLToPath(new URL("../validate.mjs", import.meta.url));

// A complete five-dimension Risk Coverage Matrix table shared by every ADR
// fixture so the dimension-coverage check has a valid baseline to perturb.
const RISK_MATRIX_TABLE = `| Risk dimension | Notes |
| --- | --- |
| concurrency and ordering | covered |
| timeout and deadline | covered |
| cancellation and interruption | covered |
| resource bounds and backpressure | covered |
| framework or trust-boundary rejection | covered`;

const ACCEPTED_RISK_MATRIX_TABLE = `| Risk dimension | Applicability and scenario, or specific N/A reason | Owning boundary | Deterministic verification method | Exact expected result | Acceptance check IDs | Status | Actual evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| concurrency and ordering | Applicable — concurrent calls. | Validator | Run the validator. | One valid result. | AC-1 | Not Started | Not run — implementation not started. |
| timeout and deadline | Applicable — deadline boundary. | Validator | Run the validator. | Timely result. | AC-1 | Not Started | Not run — implementation not started. |
| cancellation and interruption | Applicable — interrupted work. | Validator | Run the validator. | Controlled terminal. | AC-1 | Not Started | Not run — implementation not started. |
| resource bounds and backpressure | Applicable — oversized input. | Validator | Run the validator. | Input is bounded. | AC-1 | Not Started | Not run — implementation not started. |
| framework or trust-boundary rejection | Applicable — invalid input. | Validator | Run the validator. | Input is rejected. | AC-1 | Not Started | Not run — implementation not started. |`;

function write(root, path, content) {
  const absolute = join(root, path);
  mkdirSync(dirname(absolute), { recursive: true });
  writeFileSync(absolute, content);
}

function validRepository() {
  const root = mkdtempSync(join(tmpdir(), "koduck-governance-"));
  write(
    root,
    "docs/adr/INDEX.md",
    `| Type | ID | Title | Decision Status | Implementation Status | Scope | Architecture Source | Path | Superseded By |\n| --- | --- | --- | --- | --- | --- | --- | --- | --- |\n| Full ADR | ADR-0001 | Example | Proposed | Not Started | Project | N/A — governance-only example | docs/adr/ADR-0001-example.md | None |\n`,
  );
  write(
    root,
    "docs/architecture/INDEX.md",
    `| ID | Title | Design Status | Scope Level | Scope | Path | Trello Source | Superseded By |\n| --- | --- | --- | --- | --- | --- | --- | --- |\n| ADD-0001 | Example | Draft | Repository / Cross-project | Project | docs/architecture/ADD-0001-example.md | https://example.test/card | None |\n`,
  );
  write(
    root,
    "docs/adr/ADR-0001-example.md",
    `# ADR-0001: Example

## Metadata [Required]
- **Decision Status**: Proposed
- **Implementation Status**: Not Started
- **Author**: @codex
- **Decision Owner**: @linhai
- **Required Approver**: @linhai
- **Record Scope**: Project
- **Architecture Source**: N/A — governance-only example
- **Superseded By**: None

## Requirement Level Legend [Required]
Complete.
## Context [Required]
Context.
## Scope [Required]
Scope.
## Tensions, Constraints, And Open Questions [Required]
Tensions.
## Decision Drivers [Required]
Drivers.
## Options Considered [Required]
Options.
## Decision [Required]
Decision.
## Implementation Plan [Required]
Plan.
## Contract-To-Check Traceability [Required]
Traceability.
## Risk Coverage Matrix [Required]
${RISK_MATRIX_TABLE}
## Acceptance Checks [Required]
Checks.
## Completion Checklist [Required]
Checklist.
## Archival [Conditionally Required — retired]
Inactive guidance.
## Change Log [Required]
Initial.
`,
  );
  write(
    root,
    "docs/architecture/ADD-0001-example.md",
    `# ADD-0001: Example

## Metadata [Required]
- **Design Status**: Draft
- **Author**: @codex
- **Architecture Owner**: @linhai
- **Required Approver**: @linhai
- **Trello Sources**: https://example.test/card
- **Superseded By**: None
- **Scope Level**: Repository / Cross-project
- **Scope**: Project

## Requirement Level Legend [Required]
Complete.
## Context And Solution Summary [Required]
Context.
## Requirement Baseline [Required]
Baseline.
## Goals And Non-Goals [Required]
Goals.
## Functional Capability Design [Required]
Capabilities.
## Data Model Design [Conditionally Required — data changes]
N/A — this fixture creates or changes no data.
## Architecture Design [Required]
| ID | Component |
| --- | --- |
| C-1 | Component |

\`\`\`mermaid
flowchart LR
  C1["C-1"]
\`\`\`

## Control Flow Design [Conditionally Required — multi-step behavior]
N/A — this fixture has no multi-step control flow.
## Interaction Flow Design [Conditionally Required — external interaction]
N/A — this fixture has no human or external-system interaction.
## Cross-Cutting Design [Required]
Cross-cutting concerns.
## Assumptions And Open Questions [Conditionally Required — assumptions exist]
N/A — this fixture has no assumptions or open questions.
## Risks And Trade-Offs [Required]
Risks.
## ADR Task Candidates [Required]
| ID | Status | ADR path |
| --- | --- | --- |
| CAND-1 | Ready | None |

## Traceability [Required]
Traceability.
## Approval And Review Checklist [Required]
Checklist.
## Archival [Conditionally Required — retired]
Inactive guidance.
## Change Log [Required]
Initial.
`,
  );
  return root;
}

function run(root) {
  return spawnSync(process.execPath, [validator, "--root", root], {
    encoding: "utf8",
  });
}

// Replaces the body of one top-level Markdown section in a fixture.
function replaceSection(content, section, replacement) {
  const heading = `## ${section}`;
  const start = content.indexOf(heading);
  assert.notEqual(start, -1, `fixture contains ${heading}`);
  const headingEnd = content.indexOf("\n", start);
  const end = content.indexOf("\n## ", headingEnd + 1);
  return `${content.slice(0, headingEnd + 1)}${replacement}\n${end === -1 ? "" : content.slice(end + 1)}`;
}

// Writes a Complete OCR that is valid except for the aspect the caller mutates.
function completeOcr(root, id) {
  write(
    root,
    `docs/adr/ocr/OCR-${id}-example.md`,
    `# OCR-${id}-example: Example

## Metadata [Required]
- **Decision Status**: Accepted
- **Implementation Status**: Complete
- **Author**: @codex
- **Decision Owner**: @linhai
- **Required Approver**: @linhai
- **Record Scope**: Project
- **Operation Type**: Existing Runbook
- **Target Scope / Operation Owner**: Local fixture / @linhai
- **Input Source or Version**: Test fixture revision
- **Expected Output or Target State**: Fixture operation completes
- **Architecture Source**: N/A — example
- **Approver**: @linhai
- **Approval Time**: 2026-08-13T00:00:00Z
- **Approval Evidence**: Approve
- **Superseded By**: None

## Requirement Level Legend [Required]
Complete.
## Task Definition [Required]
| ID | Objective | Included scope | Completion criterion | Expected evidence | Status | Actual evidence |
| --- | --- | --- | --- | --- | --- | --- |
| T-1 | obj | scope | criterion | evidence | Complete | done |
## Eligibility [Required]

- [x] Uses an accepted architecture, pipeline, artifact contract, security boundary, and data boundary.
- [x] Is reversible; build-only recovery is artifact quarantine or discard plus no-promotion evidence.
- [x] Does not modify a Dockerfile, Makefile, CI, pipeline, artifact format, signing, credentials, deployment topology, API/schema/protocol, authentication, security policy, data lifecycle, dependency, provider, or irreversible behavior.
- [x] Has a defined preflight, success check, stop condition, and recovery/rollback path.
- [x] Contains no secret, credential, private endpoint, or sensitive user data.
- [x] Automatic-review mechanism covers the exact declared input revision.
## Core Runbook And Evidence [Required]
### Preflight [Required]
**Actual result and stable evidence**: Pass — preflight completed.
### Execute [Required]
**Actual result and stable evidence**: Pass — execute completed.
### Verify [Required]
**Actual result and stable evidence**: Pass — verify completed.
### Stop and Recovery [Required]
**Actual result and stable evidence**: Not triggered — recovery was not needed.
## Conditional Extensions [Conditionally Required — production or downstream impact]
N/A — this fixture has no production, multi-environment, phased, downstream, SLO, or change-window impact.
## Closure [Required]
- **Final result**: Completed and not promoted.
- **Authorization review**: Pass — approved by @linhai.
- **Subtask and evidence review**: Pass — T-1 is Complete with evidence.
- **Requirement-level review**: Pass — all required fields are complete.
- **Governance validation**: Pass — governance validation passed.
## Archival [Conditionally Required — retired]
Inactive guidance.
## Change Log [Required]
Initial.
`,
  );
  const indexPath = join(root, "docs/adr/INDEX.md");
  writeFileSync(
    indexPath,
    readFileSync(indexPath, "utf8").concat(
      `| OCR | OCR-${id} | Example | Accepted | Complete | Project | N/A — example | docs/adr/ocr/OCR-${id}-example.md | None |\n`,
    ),
  );
}

// Writes an Accepted, in-progress OCR whose planned fields are complete.
function acceptedOcr(root, id) {
  write(
    root,
    `docs/adr/ocr/OCR-${id}-example.md`,
    `# OCR-${id}-example: Example

## Metadata [Required]
- **Decision Status**: Accepted
- **Implementation Status**: In Progress
- **Author**: @codex
- **Decision Owner**: @linhai
- **Required Approver**: @linhai
- **Record Scope**: Project
- **Operation Type**: Existing Runbook
- **Target Scope / Operation Owner**: Local fixture / @linhai
- **Input Source or Version**: Test fixture revision
- **Expected Output or Target State**: Fixture operation completes
- **Architecture Source**: N/A — example
- **Approver**: @linhai
- **Approval Time**: 2026-08-13T00:00:00Z
- **Approval Evidence**: Approve
- **Superseded By**: None

## Requirement Level Legend [Required]
Complete.
## Task Definition [Required]
| ID | Objective | Included scope | Completion criterion | Expected evidence | Status | Actual evidence |
| --- | --- | --- | --- | --- | --- | --- |
| T-1 | obj | scope | criterion | evidence | In Progress | Pending |
## Eligibility [Required]

- [x] Uses an accepted architecture, pipeline, artifact contract, security boundary, and data boundary.
- [x] Is reversible; build-only recovery is artifact quarantine or discard plus no-promotion evidence.
- [x] Does not modify a Dockerfile, Makefile, CI, pipeline, artifact format, signing, credentials, deployment topology, API/schema/protocol, authentication, security policy, data lifecycle, dependency, provider, or irreversible behavior.
- [x] Has a defined preflight, success check, stop condition, and recovery/rollback path.
- [x] Contains no secret, credential, private endpoint, or sensitive user data.
- [x] Automatic-review mechanism covers the exact declared input revision.
## Core Runbook And Evidence [Required]
### Preflight [Required]
**Planned action and criterion**: Verify the precondition.
**Actual result and stable evidence**: Pending
### Execute [Required]
**Planned action**: Execute the operation.
**Actual result and stable evidence**: Pending
### Verify [Required]
**Success criterion**: Confirm the expected result.
**Actual result and stable evidence**: Pending
### Stop and Recovery [Required]
**Stop condition**: Stop when the precondition fails.
**Recovery action**: Restore the baseline.
**Recovery verification**: Confirm the baseline.
**Actual result and stable evidence**: Pending
## Conditional Extensions [Conditionally Required — production or downstream impact]
N/A — this fixture has no production, multi-environment, phased, downstream, SLO, or change-window impact.
## Closure [Required]
Closure.
## Archival [Conditionally Required — retired]
Inactive guidance.
## Change Log [Required]
Initial.
`,
  );
  const indexPath = join(root, "docs/adr/INDEX.md");
  writeFileSync(
    indexPath,
    readFileSync(indexPath, "utf8").concat(
      `| OCR | OCR-${id} | Example | Accepted | In Progress | Project | N/A — example | docs/adr/ocr/OCR-${id}-example.md | None |\n`,
    ),
  );
}

// Writes an Accepted, in-progress ADR whose planned tables are complete. Tests
// below mutate one approval-gate invariant at a time.
function acceptedAdr(root, id) {
  write(
    root,
    `docs/adr/ADR-${id}-example.md`,
    `# ADR-${id}: Example

## Metadata [Required]
- **Decision Status**: Accepted
- **Implementation Status**: In Progress
- **Author**: @codex
- **Decision Owner**: @linhai
- **Required Approver**: @linhai
- **Record Scope**: Project
- **Architecture Source**: N/A — governance-only example
- **Approver**: @linhai
- **Approval Time**: 2026-08-13T00:00:00Z
- **Approval Evidence**: Approve
- **Superseded By**: None

## Requirement Level Legend [Required]
Complete.
## Context [Required]
Context.
## Scope [Required]
Scope.
## Tensions, Constraints, And Open Questions [Required]
None.
## Decision Drivers [Required]
Drivers.
## Options Considered [Required]
Options.
## Decision [Required]
Decision.
## Implementation Plan [Required]
| ID | Objective | Included scope | Status | Actual implementation evidence |
| --- | --- | --- | --- | --- |
| T-1 | Implement the rule | validator | Not Started | Pending |
### Stable Implementation Touchpoints [Conditionally Required — source or configuration implementation]
| Path | Stable symbol or contract anchor | Key code excerpt, when needed | Purpose | Source revision |
| --- | --- | --- | --- | --- |
| tools/governance-validator/validate.mjs | validateRequiredBodyContent | N/A — stable symbol is sufficient | Enforce accepted-record evidence. | working tree before commit |
## Contract-To-Check Traceability [Required]
| Clause ID | Authoritative contract path and heading | Exact normative requirement | Acceptance check or deterministic test IDs | Explicit coverage method |
| --- | --- | --- | --- | --- |
| TC-1 | ADR | The rule is enforced. | AC-1 | Run the validator. |
## Risk Coverage Matrix [Required]
${ACCEPTED_RISK_MATRIX_TABLE}
## Acceptance Checks [Required]
| Check ID | Subtask | Binary acceptance point | Preconditions or input | Verification method | Exact expected result | Expected evidence | Status | Actual result and evidence |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| AC-1 | T-1 | The validator rejects the invalid record. | Invalid fixture. | Run the validator. | Exit status is 1. | Validator output. | Not Started | Pending |
## Completion Checklist [Required]
Checklist.
## Archival [Conditionally Required — retired]
Inactive guidance.
## Change Log [Required]
Initial.
`,
  );
  const indexPath = join(root, "docs/adr/INDEX.md");
  writeFileSync(
    indexPath,
    readFileSync(indexPath, "utf8").concat(
      `| Full ADR | ADR-${id} | Example | Accepted | In Progress | Project | N/A — governance-only example | docs/adr/ADR-${id}-example.md | None |\n`,
    ),
  );
}

export { ACCEPTED_RISK_MATRIX_TABLE, RISK_MATRIX_TABLE, acceptedAdr, acceptedOcr, completeOcr, replaceSection, run, validRepository, write };
