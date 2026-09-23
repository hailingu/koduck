// ADR: docs/adr/ADR-0016-adr-rejection-reason.md

import assert from "node:assert/strict";
import { readFileSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import test from "node:test";

import { RISK_MATRIX_TABLE, acceptedAdr, run, validRepository, write } from "./fixtures.mjs";

// A concrete explanation that must satisfy the ADR rejection-reason gate.
const COMPLETE_REASON = "the proposal exceeded the approved concurrency scope";

// Every fixture root is task-owned temporary state and is removed after its
// test (ADR-0016 Implementation Plan).
function repository(t) {
  const root = validRepository();
  t.after(() => rmSync(root, { recursive: true, force: true }));
  return root;
}

function reasonDiagnostic(path) {
  return new RegExp(`${path.replaceAll(/[-/.*+?^${}()|[\]\\]/g, "\\$&")}: ADR_REJECTION_REASON_REQUIRED —`);
}

// Writes an otherwise-valid archived Rejected ADR. `reason` supplies the
// active Rejection Reason value; the sentinel "@@absent@@" omits the field,
// `labeled` writes it with its requirement-level suffix, and the remaining
// options cover the lightweight type, the service ADR root, non-archived
// residence, and the independent rejection-evidence gates.
function rejectedAdr(root, {
  id,
  reason = COMPLETE_REASON,
  labeled = false,
  lightweight = false,
  service = false,
  archived = true,
  implementation = "Not Applicable",
  omitRejector = false,
}) {
  const dir = service ? "demo-service/docs/adr" : "docs/adr";
  const path = `${archived ? `${dir}/archive` : dir}/ADR-${id}-example.md`;
  const title = lightweight ? `Lightweight ADR-${id}: Example` : `ADR-${id}: Example`;
  const label = labeled ? " [Conditionally Required — Decision Status is `Rejected`]" : "";
  const rejectorLine = omitRejector ? "" : "- **Rejector**: @linhai\n";
  const reasonLine = reason === "@@absent@@" ? "" : `- **Rejection Reason${label}**: ${reason}\n`;
  write(
    root,
    path,
    `# ${title}

## Metadata [Required]
- **Decision Status**: Rejected
- **Implementation Status**: ${implementation}
- **Author**: @codex
- **Decision Owner**: @linhai
- **Required Approver**: @linhai
- **Record Scope**: Project
- **Architecture Source**: N/A — governance-only example
${rejectorLine}- **Rejection Time**: 2026-08-13T00:00:00Z
- **Rejection Evidence**: Reject
${reasonLine}- **Superseded By**: None

## Requirement Level Legend [Required]
Complete.
## Context [Required]
Context.
## Scope [Required]
Scope.
## Lightweight Eligibility Check [Required]
Eligible.
## Tensions, Constraints, And Open Questions [Required]
None.
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
  const type = lightweight ? "Lightweight ADR" : "Full ADR";
  const indexPath = join(root, "docs/adr/INDEX.md");
  writeFileSync(
    indexPath,
    readFileSync(indexPath, "utf8").concat(
      `| ${type} | ADR-${id} | Example | Rejected | ${implementation} | Project | N/A — governance-only example | ${path} | None |\n`,
    ),
  );
  return path;
}

// RR-2 (S-1): a Rejected Full ADR without an active Rejection Reason must
// fail validation with the path-prefixed diagnostic contract.
test("RR-2 rejects a Rejected Full ADR whose Rejection Reason is missing", (t) => {
  const root = repository(t);
  const path = rejectedAdr(root, { id: "0101", reason: "@@absent@@" });

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, reasonDiagnostic(path));
});

// Writes an otherwise-valid archived Rejected OCR with complete rejection
// evidence and no Rejection Reason; the ADR-only gate must leave it valid.
function rejectedOcr(root, id) {
  const path = `docs/adr/ocr/archive/OCR-${id}-example.md`;
  write(
    root,
    path,
    `# OCR-${id}-example: Example

## Metadata [Required]
- **Decision Status**: Rejected
- **Implementation Status**: Not Applicable
- **Author**: @codex
- **Decision Owner**: @linhai
- **Required Approver**: @linhai
- **Record Scope**: Project
- **Operation Type**: Existing Runbook
- **Target Scope / Operation Owner**: Local fixture / @linhai
- **Input Source or Version**: Test fixture revision
- **Expected Output or Target State**: Fixture operation completes
- **Architecture Source**: N/A — example
- **Rejector**: @linhai
- **Rejection Time**: 2026-08-13T00:00:00Z
- **Rejection Evidence**: Reject
- **Superseded By**: None

## Requirement Level Legend [Required]
Complete.
## Task Definition [Required]
Definition.
## Eligibility [Required]
Eligibility.
## Core Runbook And Evidence [Required]
Runbook.
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
      `| OCR | OCR-${id} | Example | Rejected | Not Applicable | Project | N/A — example | ${path} | None |\n`,
    ),
  );
  return path;
}

// Rewrites one fixture file through a deterministic text transformation.
function rewrite(root, path, transform) {
  const absolute = join(root, path);
  writeFileSync(absolute, transform(readFileSync(absolute, "utf8")));
}

// RR-2 failure group: every incomplete value category must fail the gate.
for (const [id, label, reason] of [
  ["0102", "an empty", ""],
  ["0103", "a whitespace-only", "   "],
  ["0104", "a Pending", "Pending"],
  ["0105", "an annotated Pending", "Pending — replace with a concrete explanation"],
  ["0106", "an N/A", "N/A — Decision Status is not Rejected"],
  ["0107", "a template-placeholder", "{{REJECTION_REASON}}"],
]) {
  test(`RR-2 rejects a Rejected Full ADR with ${label} Rejection Reason`, (t) => {
    const root = repository(t);
    const path = rejectedAdr(root, { id, reason });

    const result = run(root);
    assert.equal(result.status, 1);
    assert.match(result.stderr, reasonDiagnostic(path));
  });
}

// RR-2 failure group: the gate covers the Lightweight type and both ADR roots.
for (const [id, lightweight, service] of [
  ["0110", true, false],
  ["0111", false, true],
  ["0112", true, true],
]) {
  test(`RR-2 rejects a missing Rejection Reason for a Rejected ${lightweight ? "Lightweight" : "Full"} ADR in ${service ? "a service" : "the project"} archive`, (t) => {
    const root = repository(t);
    const path = rejectedAdr(root, { id, reason: "@@absent@@", lightweight, service });

    const result = run(root);
    assert.equal(result.status, 1);
    assert.match(result.stderr, reasonDiagnostic(path));
  });
}

// RR-2 success group (S-2): concrete reasons pass for both types, label forms,
// and scopes.
test("RR-2 accepts a Rejected Full ADR with a concrete plain Rejection Reason", (t) => {
  const root = repository(t);
  rejectedAdr(root, { id: "0120" });

  const result = run(root);
  assert.equal(result.status, 0, result.stderr);
});

test("RR-2 accepts a Rejected Lightweight ADR with a requirement-labeled reason in a service archive", (t) => {
  const root = repository(t);
  rejectedAdr(root, { id: "0121", labeled: true, lightweight: true, service: true });

  const result = run(root);
  assert.equal(result.status, 0, result.stderr);
});

// RR-2 recovery group (S-6): correcting only the missing reason restores
// validity without any state outside the document.
test("RR-2 restores validity after the missing reason is corrected", (t) => {
  const root = repository(t);
  const path = rejectedAdr(root, { id: "0122", reason: "@@absent@@" });
  assert.equal(run(root).status, 1);

  rewrite(root, path, (content) => content.replace(
    "- **Superseded By**: None",
    `- **Rejection Reason**: ${COMPLETE_REASON}\n- **Superseded By**: None`,
  ));
  const result = run(root);
  assert.equal(result.status, 0, result.stderr);
});

// RR-3 group (S-3): only the unique active Metadata value satisfies the gate.
for (const [id, label, inject] of [
  [
    "0130",
    "an HTML comment",
    (reason) => (content) => content.replace(
      "- **Superseded By**: None",
      `<!--\n- **Rejection Reason**: ${reason}\n-->\n- **Superseded By**: None`,
    ),
  ],
  [
    "0131",
    "a fenced code block",
    (reason) => (content) => content.replace(
      "- **Superseded By**: None",
      "```\n" + `- **Rejection Reason**: ${reason}` + "\n```\n- **Superseded By**: None",
    ),
  ],
  [
    "0132",
    "the Change Log",
    (reason) => (content) => content.replace(
      "## Change Log [Required]\nInitial.",
      `## Change Log [Required]\nInitial.\n- **Rejection Reason**: ${reason}`,
    ),
  ],
  [
    "0133",
    "a narrative section",
    (reason) => (content) => content.replace(
      "## Context [Required]\nContext.",
      `## Context [Required]\nContext.\n- **Rejection Reason**: ${reason}`,
    ),
  ],
]) {
  test(`RR-3 does not satisfy the reason from ${label}`, (t) => {
    const root = repository(t);
    const path = rejectedAdr(root, { id, reason: "@@absent@@" });
    rewrite(root, path, inject(COMPLETE_REASON));

    const result = run(root);
    assert.equal(result.status, 1);
    assert.match(result.stderr, reasonDiagnostic(path));
  });
}

test("RR-3 does not satisfy the reason from duplicate active metadata and retains the duplicate diagnostic", (t) => {
  const root = repository(t);
  const path = rejectedAdr(root, { id: "0134", reason: "@@absent@@" });
  rewrite(root, path, (content) => content.replace(
    "- **Superseded By**: None",
    `- **Rejection Reason**: ${COMPLETE_REASON}\n- **Rejection Reason**: a contradictory second explanation\n- **Superseded By**: None`,
  ));

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, reasonDiagnostic(path));
  assert.match(result.stderr, /metadata field Rejection Reason must appear exactly once; found 2/);
});

// RR-4 compatibility group (S-4): the gate applies only to Rejected ADRs.
test("RR-4 leaves a Proposed ADR without the field valid", (t) => {
  const root = repository(t);

  const result = run(root);
  assert.equal(result.status, 0, result.stderr);
});

test("RR-4 leaves an Accepted ADR without the field valid", (t) => {
  const root = repository(t);
  acceptedAdr(root, "0140");

  const result = run(root);
  assert.equal(result.status, 0, result.stderr);
});

test("RR-4 leaves a Rejected OCR without the field valid", (t) => {
  const root = repository(t);
  rejectedOcr(root, "0141");

  const result = run(root);
  assert.equal(result.status, 0, result.stderr);
});

// RR-4 group (S-5): a complete reason never substitutes for the independent
// rejection, final-state, and archival-residence gates.
test("RR-4 still requires the Rejector metadata when the reason is complete", (t) => {
  const root = repository(t);
  rejectedAdr(root, { id: "0142", omitRejector: true });

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /Rejected requires complete Rejector, Rejection Time, and Rejection Evidence: Reject metadata/);
});

test("RR-4 still requires Not Applicable implementation for a Rejected ADR with a complete reason", (t) => {
  const root = repository(t);
  rejectedAdr(root, { id: "0143", implementation: "In Progress" });

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /Rejected requires Implementation Status Not Applicable/);
});

test("RR-4 still requires archive residence for a Rejected ADR with a complete reason", (t) => {
  const root = repository(t);
  rejectedAdr(root, { id: "0144", archived: false });

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /a Rejected\/Not Applicable record must reside under an archive\/ directory/);
});
