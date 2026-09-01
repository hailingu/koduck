// ADR: docs/adr/ADR-0002-required-ai-ci-postgres-verification.md

import assert from "node:assert/strict";
import { mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import test from "node:test";
import { acceptedAdr, completeOcr, replaceSection, run, validRepository } from "./fixtures.mjs";

// The template-defined Completion Checklist required for a terminal ADR.
const COMPLETION_CHECKLIST_TABLE = `| ID | Item | Status | Actual evidence |
| --- | --- | --- | --- |
| A-1 | Approval recorded. | Complete | Approve recorded. |
| A-2 | Subtasks terminal. | Complete | T-1 Complete. |
| A-3 | Checks pass when applicable. | Complete | AC-1 Pass. |
| A-4 | Traceability complete. | Complete | TC-1 mapped. |
| A-5 | Risk matrix complete. | Complete | Every row Pass. |
| A-6 | Evidence complete when applicable. | Complete | Evidence stored. |
| A-7 | Standards checked. | Complete | Standards reviewed. |
| A-8 | Report complete. | Complete | Reported. |`;

// Writes an Accepted ADR whose terminal evidence is complete, for tests that
// perturb exactly one terminal-evidence gate.
function completeAdr(root, id = "0002") {
  acceptedAdr(root, id);
  const path = join(root, `docs/adr/ADR-${id}-example.md`);
  let markdown = readFileSync(path, "utf8");
  markdown = markdown.replace(
    "- **Implementation Status**: In Progress",
    "- **Implementation Status**: Complete",
  );
  markdown = markdown.replace(
    "| T-1 | Implement the rule | validator | Not Started | Pending |",
    "| T-1 | Implement the rule | validator | Complete | Implemented and recorded. |",
  );
  markdown = markdown.replace(
    "| Validator output. | Not Started | Pending |",
    "| Validator output. | Pass | Exit status 0 observed. |",
  );
  markdown = markdown.replaceAll(
    "| AC-1 | Not Started | Not run — implementation not started. |",
    "| AC-1 | Pass | Deterministic run recorded. |",
  );
  markdown = replaceSection(markdown, "Completion Checklist", COMPLETION_CHECKLIST_TABLE);
  writeFileSync(path, markdown);
  const indexPath = join(root, "docs/adr/INDEX.md");
  writeFileSync(
    indexPath,
    readFileSync(indexPath, "utf8").replace(
      `| ADR-${id} | Example | Accepted | In Progress |`,
      `| ADR-${id} | Example | Accepted | Complete |`,
    ),
  );
  return path;
}

test("accepts a fully evidenced Complete ADR", () => {
  const root = validRepository();
  completeAdr(root);
  const result = run(root);
  assert.equal(result.status, 0, result.stderr);
});

test("rejects a terminal ADR without an Implementation Plan table", () => {
  const root = validRepository();
  const path = completeAdr(root);
  writeFileSync(path, replaceSection(
    readFileSync(path, "utf8"),
    "Implementation Plan",
    "The plan is described in prose without a table.",
  ));
  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /Implementation Plan requires a structured table for a terminal record/);
});

test("rejects a terminal ADR without an Acceptance Checks table", () => {
  const root = validRepository();
  const path = completeAdr(root);
  writeFileSync(path, replaceSection(
    readFileSync(path, "utf8"),
    "Acceptance Checks",
    "Checks are described in prose without a table.",
  ));
  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /Acceptance Checks requires a structured table for a terminal record/);
});

test("rejects a terminal subtask row with an illegal ID", () => {
  const root = validRepository();
  const path = completeAdr(root);
  writeFileSync(
    path,
    readFileSync(path, "utf8").replace("| T-1 | Implement the rule", "| X-1 | Implement the rule"),
  );
  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /Implementation Plan table has a row with a missing or illegal ID \(X-1\)/);
});

test("rejects a duplicated terminal subtask ID", () => {
  const root = validRepository();
  const path = completeAdr(root);
  writeFileSync(
    path,
    readFileSync(path, "utf8").replace(
      "| T-1 | Implement the rule | validator | Complete | Implemented and recorded. |",
      "| T-1 | Implement the rule | validator | Complete | Implemented and recorded. |\n| T-1 | Repeat the rule | validator | Complete | Repeated. |",
    ),
  );
  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /Implementation Plan table duplicates subtask ID T-1/);
});

test("rejects a terminal subtask that is not Complete", () => {
  const root = validRepository();
  const path = completeAdr(root);
  writeFileSync(
    path,
    readFileSync(path, "utf8").replace(
      "| T-1 | Implement the rule | validator | Complete | Implemented and recorded. |",
      "| T-1 | Implement the rule | validator | In Progress | Implemented and recorded. |",
    ),
  );
  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /subtask T-1 must be Complete or N\/A for a terminal record \(is In Progress\)/);
});

test("rejects a terminal acceptance check that is not Pass", () => {
  const root = validRepository();
  const path = completeAdr(root);
  writeFileSync(
    path,
    readFileSync(path, "utf8").replace("| Validator output. | Pass |", "| Validator output. | Fail |"),
  );
  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /acceptance check AC-1 must be Pass or N\/A for a terminal record \(is Fail\)/);
});

test("rejects a Passing terminal acceptance check without actual evidence", () => {
  const root = validRepository();
  const path = completeAdr(root);
  writeFileSync(
    path,
    readFileSync(path, "utf8").replace(
      "| Validator output. | Pass | Exit status 0 observed. |",
      "| Validator output. | Pass | Pending |",
    ),
  );
  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /acceptance check AC-1 must record actual completion evidence \(is Pending\)/);
});

test("rejects a terminal Completion Checklist missing an item", () => {
  const root = validRepository();
  const path = completeAdr(root);
  writeFileSync(
    path,
    readFileSync(path, "utf8").replace("| A-3 | Checks pass when applicable. | Complete | AC-1 Pass. |\n", ""),
  );
  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /Completion Checklist item A-3 is missing for a terminal record/);
});

test("rejects a duplicated Completion Checklist item", () => {
  const root = validRepository();
  const path = completeAdr(root);
  writeFileSync(
    path,
    readFileSync(path, "utf8").replace(
      "| A-8 | Report complete. | Complete | Reported. |",
      "| A-8 | Report complete. | Complete | Reported. |\n| A-8 | Report again. | Complete | Reported. |",
    ),
  );
  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /Completion Checklist duplicates item A-8/);
});

test("rejects a terminal ADR whose A-1 approval item is not Complete", () => {
  const root = validRepository();
  const path = completeAdr(root);
  writeFileSync(
    path,
    readFileSync(path, "utf8").replace(
      "| A-1 | Approval recorded. | Complete | Approve recorded. |",
      "| A-1 | Approval recorded. | N/A — not needed | Not recorded. |",
    ),
  );
  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /Completion Checklist item A-1 must be Complete for a terminal record/);
});

test("rejects a terminal risk dimension that is not Pass", () => {
  const root = validRepository();
  const path = completeAdr(root);
  writeFileSync(
    path,
    readFileSync(path, "utf8").replace(
      "| concurrency and ordering | Applicable — concurrent calls. | Validator | Run the validator. | One valid result. | AC-1 | Pass | Deterministic run recorded. |",
      "| concurrency and ordering | Applicable — concurrent calls. | Validator | Run the validator. | One valid result. | AC-1 | Fail | Deterministic run recorded. |",
    ),
  );
  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /Risk Coverage Matrix dimension concurrency and ordering must be Pass or N\/A for a terminal record \(is Fail\)/);
});

test("rejects a Passing terminal risk row without stable evidence", () => {
  const root = validRepository();
  const path = completeAdr(root);
  writeFileSync(
    path,
    readFileSync(path, "utf8").replace(
      "| timeout and deadline | Applicable — deadline boundary. | Validator | Run the validator. | Timely result. | AC-1 | Pass | Deterministic run recorded. |",
      "| timeout and deadline | Applicable — deadline boundary. | Validator | Run the validator. | Timely result. | AC-1 | Pass | Pending |",
    ),
  );
  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /Risk Coverage Matrix dimension timeout and deadline Pass row must record stable evidence/);
});

// Makes the Complete OCR fixture fully valid at the terminal gate: adds each
// runbook stage's planned fields and moves the record into the required
// archive/ directory (AGENTS.md archival residence).
function archiveTerminalOcr(root, id) {
  const path = join(root, `docs/adr/ocr/OCR-${id}-example.md`);
  let markdown = readFileSync(path, "utf8");
  markdown = markdown.replace(
    "**Actual result and stable evidence**: Pass — preflight completed.",
    "**Planned action and criterion**: Verify preconditions.\n**Actual result and stable evidence**: Pass — preflight completed.",
  );
  markdown = markdown.replace(
    "**Actual result and stable evidence**: Pass — execute completed.",
    "**Planned action**: Execute the operation.\n**Actual result and stable evidence**: Pass — execute completed.",
  );
  markdown = markdown.replace(
    "**Actual result and stable evidence**: Pass — verify completed.",
    "**Success criterion**: Expected result confirmed.\n**Actual result and stable evidence**: Pass — verify completed.",
  );
  markdown = markdown.replace(
    "**Actual result and stable evidence**: Not triggered — recovery was not needed.",
    "**Stop condition**: Stop on failure.\n**Recovery action**: Restore the baseline.\n**Recovery verification**: Baseline confirmed.\n**Actual result and stable evidence**: Not triggered — recovery was not needed.",
  );
  const archivedPath = join(root, `docs/adr/ocr/archive/OCR-${id}-example.md`);
  mkdirSync(dirname(archivedPath), { recursive: true });
  writeFileSync(archivedPath, markdown);
  rmSync(path);
  const indexPath = join(root, "docs/adr/INDEX.md");
  writeFileSync(
    indexPath,
    readFileSync(indexPath, "utf8").replace(
      `docs/adr/ocr/OCR-${id}-example.md`,
      `docs/adr/ocr/archive/OCR-${id}-example.md`,
    ),
  );
}

test("accepts a fully evidenced Complete OCR", () => {
  const root = validRepository();
  completeOcr(root, "0002");
  archiveTerminalOcr(root, "0002");
  const result = run(root);
  assert.equal(result.status, 0, result.stderr);
});

test("rejects a terminal OCR runbook stage without actual evidence", () => {
  const root = validRepository();
  completeOcr(root, "0002");
  const path = join(root, "docs/adr/ocr/OCR-0002-example.md");
  writeFileSync(
    path,
    readFileSync(path, "utf8").replace(
      "**Actual result and stable evidence**: Pass — verify completed.",
      "**Actual result and stable evidence**: Pending",
    ),
  );
  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /stage Verify must record actual result evidence for a terminal OCR/);
});

test("rejects a terminal OCR whose Governance validation review is not Pass", () => {
  const root = validRepository();
  completeOcr(root, "0002");
  const path = join(root, "docs/adr/ocr/OCR-0002-example.md");
  writeFileSync(
    path,
    readFileSync(path, "utf8").replace(
      "- **Governance validation**: Pass — governance validation passed.",
      "- **Governance validation**: Fail — governance validation failed.",
    ),
  );
  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /Closure field Governance validation must be Pass for a terminal OCR/);
});

test("rejects a terminal OCR without a complete Final result", () => {
  const root = validRepository();
  completeOcr(root, "0002");
  const path = join(root, "docs/adr/ocr/OCR-0002-example.md");
  writeFileSync(
    path,
    readFileSync(path, "utf8").replace(
      "- **Final result**: Completed and not promoted.",
      "- **Final result**: Pending",
    ),
  );
  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /Closure field Final result must record a complete terminal result for a terminal OCR/);
});
