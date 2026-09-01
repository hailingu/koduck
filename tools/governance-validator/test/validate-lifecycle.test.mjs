// ADR: docs/adr/ADR-0014-validator-structural-parsing-reliability.md

import assert from "node:assert/strict";
import { readFileSync, unlinkSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import test from "node:test";
import { ACCEPTED_RISK_MATRIX_TABLE, RISK_MATRIX_TABLE, acceptedAdr, acceptedOcr, completeOcr, replaceSection, run, validRepository, write } from "./validate.test.mjs";
test("rejects Pending as a Retirement Reason", () => {
  const root = validRepository();
  const path = join(root, "docs/adr/ADR-0001-example.md");
  writeFileSync(
    path,
    readFileSync(path, "utf8")
      .replace("Decision Status**: Proposed", "Decision Status**: Deprecated")
      .replace("Implementation Status**: Not Started", "Implementation Status**: Not Applicable")
      .replace(
        "Architecture Source**: N/A — governance-only example",
        "Architecture Source**: N/A — governance-only example\n- **Retired By**: @codex\n- **Retirement Time**: 2026-08-13T00:00:00Z\n- **Retirement Evidence**: Deprecate\n- **Retirement Reason**: Pending",
      ),
  );
  const indexPath = join(root, "docs/adr/INDEX.md");
  writeFileSync(
    indexPath,
    readFileSync(indexPath, "utf8")
      .replace("| Proposed |", "| Deprecated |")
      .replace("| Not Started |", "| Not Applicable |"),
  );

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /retired decision requires complete Retired By.*Retirement Reason/i);
});

test("rejects a Superseded record without a replacement", () => {
  const root = validRepository();
  const path = join(root, "docs/adr/ADR-0001-example.md");
  writeFileSync(
    path,
    readFileSync(path, "utf8")
      .replace("Decision Status**: Proposed", "Decision Status**: Superseded")
      .replace("Implementation Status**: Not Started", "Implementation Status**: Not Applicable")
      .replace(
        "Architecture Source**: N/A — governance-only example",
        "Architecture Source**: N/A — governance-only example\n- **Retired By**: @codex\n- **Retirement Time**: 2026-08-13T00:00:00Z\n- **Retirement Evidence**: Supersede\n- **Retirement Reason**: replaced",
      ),
  );
  const indexPath = join(root, "docs/adr/INDEX.md");
  writeFileSync(
    indexPath,
    readFileSync(indexPath, "utf8")
      .replace("| Proposed |", "| Superseded |")
      .replace("| Not Started |", "| Not Applicable |"),
  );

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /Superseded requires a Superseded By replacement record path/i);
});

test("rejects an Author approving their own ADR", () => {
  const root = validRepository();
  const path = join(root, "docs/adr/ADR-0001-example.md");
  writeFileSync(
    path,
    readFileSync(path, "utf8")
      .replace("Decision Status**: Proposed", "Decision Status**: Accepted")
      .replace(/- \*\*Author\*\*:[^\n]+/, "- **Author**: @codex")
      .replace(
        "## Metadata [Required]",
        "## Metadata [Required]\n- **Approver**: @codex\n- **Approval Time**: 2026-08-13T00:00:00Z\n- **Approval Evidence**: Approve",
      ),
  );
  const indexPath = join(root, "docs/adr/INDEX.md");
  writeFileSync(indexPath, readFileSync(indexPath, "utf8").replace("| Proposed |", "| Accepted |"));

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /Approver must differ from Author/i);
});

test("rejects an Author approving their own ADD", () => {
  const root = validRepository();
  const addPath = join(root, "docs/architecture/ADD-0001-example.md");
  writeFileSync(
    addPath,
    readFileSync(addPath, "utf8")
      .replace("Design Status**: Draft", "Design Status**: Current")
      .replace(/- \*\*Author\*\*:[^\n]+/, "- **Author**: @codex")
      .replace(
        "## Metadata [Required]",
        "## Metadata [Required]\n- **Approver**: @codex\n- **Approval Time**: 2026-08-13T00:00:00Z\n- **Approval Evidence**: Approve",
      ),
  );
  const indexPath = join(root, "docs/architecture/INDEX.md");
  writeFileSync(indexPath, readFileSync(indexPath, "utf8").replace("| Draft |", "| Current |"));

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /Approver must differ from Author/i);
});

test("rejects a Current ADD whose Control Flow lacks a Mermaid diagram", () => {
  const root = validRepository();
  const addPath = join(root, "docs/architecture/ADD-0001-example.md");
  let content = readFileSync(addPath, "utf8")
    .replace("Design Status**: Draft", "Design Status**: Current")
    .replace(
      "## Metadata [Required]",
      "## Metadata [Required]\n- **Approver**: @linhai\n- **Approval Time**: 2026-08-13T00:00:00Z\n- **Approval Evidence**: Approve",
    );
  content = replaceSection(
    content,
    "Control Flow Design",
    "| ID | Step |\n| --- | --- |\n| CF-1 | step |",
  );
  writeFileSync(
    addPath,
    content,
  );
  const indexPath = join(root, "docs/architecture/INDEX.md");
  writeFileSync(indexPath, readFileSync(indexPath, "utf8").replace("| Draft |", "| Current |"));

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /Control Flow Design requires a Mermaid diagram for a Current ADD/i);
});

test("requires every conditional design section in a Current ADD", () => {
  for (const section of [
    "Data Model Design",
    "Control Flow Design",
    "Interaction Flow Design",
    "Assumptions And Open Questions",
  ]) {
    const root = validRepository();
    const addPath = join(root, "docs/architecture/ADD-0001-example.md");
    let content = readFileSync(addPath, "utf8")
      .replace("Design Status**: Draft", "Design Status**: Current")
      .replace(
        "## Metadata [Required]",
        "## Metadata [Required]\n- **Approver**: @linhai\n- **Approval Time**: 2026-08-13T00:00:00Z\n- **Approval Evidence**: Approve",
      );
    content = replaceSection(content, section, "");
    writeFileSync(addPath, content.replace(new RegExp(`## ${section}[^\\n]*\\n\\n`), ""));
    const indexPath = join(root, "docs/architecture/INDEX.md");
    writeFileSync(indexPath, readFileSync(indexPath, "utf8").replace("| Draft |", "| Current |"));

    const result = run(root);
    assert.equal(result.status, 1, `${section}: ${result.stderr || result.stdout}`);
    assert.match(result.stderr, new RegExp(`missing conditionally required section ${section}`, "i"));
  }
});

test("does not read risk dimensions from inside a code block", () => {
  const root = validRepository();
  const path = join(root, "docs/adr/ADR-0001-example.md");
  writeFileSync(
    path,
    readFileSync(path, "utf8").replace(
      /## Risk Coverage Matrix \[Required\][\s\S]*?(?=## Acceptance Checks)/,
      "## Risk Coverage Matrix [Required]\nNo real table.\n\n```\n| Risk dimension | Notes |\n| --- | --- |\n| concurrency and ordering | n |\n| timeout and deadline | n |\n| cancellation and interruption | n |\n| resource bounds and backpressure | n |\n| framework or trust-boundary rejection | n |\n```\n\n",
    ),
  );

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /Risk Coverage Matrix must contain a five-dimension table or N\/A/i);
});

test("rejects an Accepted ADR missing the Author field", () => {
  const root = validRepository();
  const path = join(root, "docs/adr/ADR-0001-example.md");
  writeFileSync(
    path,
    readFileSync(path, "utf8")
      .replace("- **Author**: @codex\n", "")
      .replace("Decision Status**: Proposed", "Decision Status**: Accepted")
      .replace(
        "Architecture Source**: N/A — governance-only example",
        "Architecture Source**: N/A — governance-only example\n- **Approver**: @linhai\n- **Approval Time**: 2026-08-13T00:00:00Z\n- **Approval Evidence**: Approve",
      ),
  );
  const indexPath = join(root, "docs/adr/INDEX.md");
  writeFileSync(indexPath, readFileSync(indexPath, "utf8").replace("| Proposed |", "| Accepted |"));

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /metadata field Author must be a concrete/i);
});

test("rejects a Complete ADR whose subtask is not Complete", () => {
  const root = validRepository();
  const path = join(root, "docs/adr/ADR-0001-example.md");
  writeFileSync(
    path,
    readFileSync(path, "utf8")
      .replace("Decision Status**: Proposed", "Decision Status**: Accepted")
      .replace("Implementation Status**: Not Started", "Implementation Status**: Complete")
      .replace(
        "## Implementation Plan [Required]\nPlan.",
        "## Implementation Plan [Required]\n| ID | Objective | Included scope | Status | Actual implementation evidence |\n| --- | --- | --- | --- | --- |\n| T-1 | obj | scope | In Progress | evidence |\n| T-2 | obj | scope | Complete | evidence |",
      )
      .replace(
        "Architecture Source**: N/A — governance-only example",
        "Architecture Source**: N/A — governance-only example\n- **Approver**: @linhai\n- **Approval Time**: 2026-08-13T00:00:00Z\n- **Approval Evidence**: Approve",
      ),
  );
  const indexPath = join(root, "docs/adr/INDEX.md");
  writeFileSync(
    indexPath,
    readFileSync(indexPath, "utf8")
      .replace("| Proposed |", "| Accepted |")
      .replace("| Not Started |", "| Complete |"),
  );

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /subtask T-1 must be Complete or N\/A/i);
});

test("rejects a Superseded By target that is not an indexed record", () => {
  const root = validRepository();
  // A translations helper that declares Accepted + a reciprocal Supersedes must
  // not satisfy the replacement requirement.
  write(
    root,
    "docs/adr/translations/zh-CN/ADR-0001-example.md",
    "# ADR-0001 translation\n\n- **Decision Status**: Accepted\n- **Supersedes**: docs/adr/ADR-0001-example.md\n",
  );
  const path = join(root, "docs/adr/ADR-0001-example.md");
  writeFileSync(
    path,
    readFileSync(path, "utf8")
      .replace("Decision Status**: Proposed", "Decision Status**: Superseded")
      .replace("Implementation Status**: Not Started", "Implementation Status**: Not Applicable")
      .replace(
        "Superseded By**: None",
        "Superseded By**: docs/adr/translations/zh-CN/ADR-0001-example.md",
      )
      .replace(
        "Architecture Source**: N/A — governance-only example",
        "Architecture Source**: N/A — governance-only example\n- **Retired By**: @codex\n- **Retirement Time**: 2026-08-13T00:00:00Z\n- **Retirement Evidence**: Supersede\n- **Retirement Reason**: replaced",
      ),
  );
  const indexPath = join(root, "docs/adr/INDEX.md");
  writeFileSync(
    indexPath,
    readFileSync(indexPath, "utf8")
      .replace("| Proposed |", "| Superseded |")
      .replace("| Not Started |", "| Not Applicable |"),
  );

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /Superseded By must name an ADR, ADD, or OCR record/i);
});

test("rejects supersession checklist and risk contracts with trailing path content", () => {
  const checklistRoot = validRepository();
  acceptedOcr(checklistRoot, "0002");
  const checklistPath = join(checklistRoot, "docs/adr/ocr/OCR-0002-example.md");
  writeFileSync(
    checklistPath,
    readFileSync(checklistPath, "utf8").replace(
      "- [x] Uses an accepted architecture",
      "\t- [x] Uses an accepted architecture",
    ),
  );
  const checklistResult = run(checklistRoot);
  assert.equal(checklistResult.status, 1);
  assert.match(checklistResult.stderr, /Eligibility item .* must be present and confirmed/i);

  const root = validRepository();
  acceptedAdr(root, "0002");
  const activePath = join(root, "docs/adr/ADR-0001-example.md");
  const archivedPath = "docs/adr/archive/ADR-0001-example.md";
  const replacementPath = join(root, "docs/adr/ADR-0002-example.md");
  writeFileSync(
    replacementPath,
    readFileSync(replacementPath, "utf8").replace(
      "Superseded By**: None",
      "Superseded By**: None\n- **Supersedes**: docs/adr/archive/ADR-0001-example.md",
    ),
  );
  const retired = readFileSync(activePath, "utf8")
    .replace("Decision Status**: Proposed", "Decision Status**: Superseded")
    .replace("Implementation Status**: Not Started", "Implementation Status**: Not Applicable")
    .replace(
      "Superseded By**: None",
      "Superseded By**: docs/adr/ADR-0002-example.md.trailing",
    )
    .replace(
      "Architecture Source**: N/A — governance-only example",
      "Architecture Source**: N/A — governance-only example\n- **Retired By**: @linhai\n- **Retirement Time**: 2026-09-01T00:00:00Z\n- **Retirement Evidence**: Supersede\n- **Retirement Reason**: replacement created",
    );
  write(root, archivedPath, retired);
  unlinkSync(activePath);
  const indexPath = join(root, "docs/adr/INDEX.md");
  writeFileSync(
    indexPath,
    readFileSync(indexPath, "utf8")
      .replace("| Full ADR | ADR-0001 | Example | Proposed | Not Started |", "| Full ADR | ADR-0001 | Example | Superseded | Not Applicable |")
      .replace("docs/adr/ADR-0001-example.md | None |", "docs/adr/archive/ADR-0001-example.md | docs/adr/ADR-0002-example.md.trailing |"),
  );

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /Superseded By.*replacement record path/i);
});

test("rejects a Complete ADR whose Implementation Plan has no table", () => {
  const root = validRepository();
  const path = join(root, "docs/adr/ADR-0001-example.md");
  writeFileSync(
    path,
    readFileSync(path, "utf8")
      .replace("Decision Status**: Proposed", "Decision Status**: Accepted")
      .replace("Implementation Status**: Not Started", "Implementation Status**: Complete")
      .replace(
        "Architecture Source**: N/A — governance-only example",
        "Architecture Source**: N/A — governance-only example\n- **Approver**: @linhai\n- **Approval Time**: 2026-08-13T00:00:00Z\n- **Approval Evidence**: Approve",
      ),
  );
  const indexPath = join(root, "docs/adr/INDEX.md");
  writeFileSync(
    indexPath,
    readFileSync(indexPath, "utf8")
      .replace("| Proposed |", "| Accepted |")
      .replace("| Not Started |", "| Complete |"),
  );

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /Implementation Plan requires a structured table for a terminal record/i);
});

test("rejects a Complete subtask whose evidence is Pending", () => {
  const root = validRepository();
  const path = join(root, "docs/adr/ADR-0001-example.md");
  writeFileSync(
    path,
    readFileSync(path, "utf8")
      .replace("Decision Status**: Proposed", "Decision Status**: Accepted")
      .replace("Implementation Status**: Not Started", "Implementation Status**: Complete")
      .replace(
        "## Implementation Plan [Required]\nPlan.",
        "## Implementation Plan [Required]\n| ID | Objective | Included scope | Status | Actual implementation evidence |\n| --- | --- | --- | --- | --- |\n| T-1 | obj | scope | Complete | Pending |",
      )
      .replace(
        "Architecture Source**: N/A — governance-only example",
        "Architecture Source**: N/A — governance-only example\n- **Approver**: @linhai\n- **Approval Time**: 2026-08-13T00:00:00Z\n- **Approval Evidence**: Approve",
      ),
  );
  const indexPath = join(root, "docs/adr/INDEX.md");
  writeFileSync(
    indexPath,
    readFileSync(indexPath, "utf8")
      .replace("| Proposed |", "| Accepted |")
      .replace("| Not Started |", "| Complete |"),
  );

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /subtask T-1 must record actual completion evidence/i);
});

test("rejects a Complete OCR whose Task Definition subtask is not Complete", () => {
  const root = validRepository();
  write(
    root,
    "docs/adr/ocr/OCR-0003-example.md",
    `# OCR-0003-example: Example

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
| T-2 | obj | scope | criterion | evidence | In Progress | Pending |
## Eligibility [Required]

- [x] Uses an accepted architecture, pipeline, artifact contract, security boundary, and data boundary.
- [x] Is reversible; build-only recovery is artifact quarantine or discard plus no-promotion evidence.
- [x] Does not modify a Dockerfile, Makefile, CI, pipeline, artifact format, signing, credentials, deployment topology, API/schema/protocol, authentication, security policy, data lifecycle, dependency, provider, or irreversible behavior.
- [x] Has a defined preflight, success check, stop condition, and recovery/rollback path.
- [x] Contains no secret, credential, private endpoint, or sensitive user data.
- [x] Automatic-review mechanism covers the exact declared input revision.
## Core Runbook And Evidence [Required]
**Actual result and stable evidence**: the operation completed and produced the locked manifest.
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
      "| OCR | OCR-0003 | Example | Accepted | Complete | Project | N/A — example | docs/adr/ocr/OCR-0003-example.md | None |\n",
    ),
  );

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /subtask T-2 must be Complete or N\/A/i);
});

test("rejects an ADD missing the Scope Level metadata", () => {
  const root = validRepository();
  const addPath = join(root, "docs/architecture/ADD-0001-example.md");
  writeFileSync(
    addPath,
    readFileSync(addPath, "utf8").replace("- **Scope Level**: Repository / Cross-project\n", ""),
  );

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /missing required metadata field Scope Level/i);
});

test("rejects a terminal table missing the actual evidence column", () => {
  const root = validRepository();
  const path = join(root, "docs/adr/ADR-0001-example.md");
  writeFileSync(
    path,
    readFileSync(path, "utf8")
      .replace("Decision Status**: Proposed", "Decision Status**: Accepted")
      .replace("Implementation Status**: Not Started", "Implementation Status**: Complete")
      .replace(
        "## Implementation Plan [Required]\nPlan.",
        "## Implementation Plan [Required]\n| ID | Objective | Status |\n| --- | --- | --- |\n| T-1 | obj | Complete |",
      )
      .replace(
        "Architecture Source**: N/A — governance-only example",
        "Architecture Source**: N/A — governance-only example\n- **Approver**: @linhai\n- **Approval Time**: 2026-08-13T00:00:00Z\n- **Approval Evidence**: Approve",
      ),
  );
  const indexPath = join(root, "docs/adr/INDEX.md");
  writeFileSync(
    indexPath,
    readFileSync(indexPath, "utf8")
      .replace("| Proposed |", "| Accepted |")
      .replace("| Not Started |", "| Complete |"),
  );

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /Implementation Plan table is missing a required actual evidence column/i);
});

test("rejects an illegal subtask ID in a terminal Implementation Plan", () => {
  const root = validRepository();
  const path = join(root, "docs/adr/ADR-0001-example.md");
  writeFileSync(
    path,
    readFileSync(path, "utf8")
      .replace("Decision Status**: Proposed", "Decision Status**: Accepted")
      .replace("Implementation Status**: Not Started", "Implementation Status**: Complete")
      .replace(
        "## Implementation Plan [Required]\nPlan.",
        "## Implementation Plan [Required]\n| ID | Objective | Status | Actual implementation evidence |\n| --- | --- | --- | --- |\n| T-1 | obj | Complete | done |\n| T-X | obj | Complete | done |",
      )
      .replace(
        "Architecture Source**: N/A — governance-only example",
        "Architecture Source**: N/A — governance-only example\n- **Approver**: @linhai\n- **Approval Time**: 2026-08-13T00:00:00Z\n- **Approval Evidence**: Approve",
      ),
  );
  const indexPath = join(root, "docs/adr/INDEX.md");
  writeFileSync(
    indexPath,
    readFileSync(indexPath, "utf8")
      .replace("| Proposed |", "| Accepted |")
      .replace("| Not Started |", "| Complete |"),
  );

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /Implementation Plan table has a row with a missing or illegal ID \(T-X\)/i);
});

test("rejects a terminal Implementation Plan with more than three subtasks", () => {
  const root = validRepository();
  const path = join(root, "docs/adr/ADR-0001-example.md");
  writeFileSync(
    path,
    readFileSync(path, "utf8")
      .replace("Decision Status**: Proposed", "Decision Status**: Accepted")
      .replace("Implementation Status**: Not Started", "Implementation Status**: Complete")
      .replace(
        "## Implementation Plan [Required]\nPlan.",
        "## Implementation Plan [Required]\n| ID | Objective | Status | Actual implementation evidence |\n| --- | --- | --- | --- |\n| T-1 | obj | Complete | done |\n| T-2 | obj | Complete | done |\n| T-3 | obj | Complete | done |\n| T-4 | obj | Complete | done |",
      )
      .replace(
        "Architecture Source**: N/A — governance-only example",
        "Architecture Source**: N/A — governance-only example\n- **Approver**: @linhai\n- **Approval Time**: 2026-08-13T00:00:00Z\n- **Approval Evidence**: Approve",
      ),
  );
  const indexPath = join(root, "docs/adr/INDEX.md");
  writeFileSync(
    indexPath,
    readFileSync(indexPath, "utf8")
      .replace("| Proposed |", "| Accepted |")
      .replace("| Not Started |", "| Complete |"),
  );

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /Implementation Plan must have at most 3 subtask rows/i);
});

test("rejects a Complete OCR whose runbook lacks required stages", () => {
  const root = validRepository();
  write(
    root,
    "docs/adr/ocr/OCR-0004-example.md",
    `# OCR-0004-example: Example

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
      "| OCR | OCR-0004 | Example | Accepted | Complete | Project | N/A — example | docs/adr/ocr/OCR-0004-example.md | None |\n",
    ),
  );

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(
    result.stderr,
    /Core Runbook And Evidence is missing required stage Execute for a terminal OCR/i,
  );
});

test("rejects a terminal ADR whose risk row is not Pass", () => {
  const root = validRepository();
  const path = join(root, "docs/adr/ADR-0001-example.md");
  writeFileSync(
    path,
    readFileSync(path, "utf8")
      .replace("Decision Status**: Proposed", "Decision Status**: Accepted")
      .replace("Implementation Status**: Not Started", "Implementation Status**: Complete")
      .replace(
        /## Risk Coverage Matrix \[Required\][\s\S]*?(?=## Acceptance Checks)/,
        "## Risk Coverage Matrix [Required]\n| Risk dimension | Status |\n| --- | --- |\n| concurrency and ordering | Not Started |\n| timeout and deadline | Pass |\n| cancellation and interruption | Pass |\n| resource bounds and backpressure | Pass |\n| framework or trust-boundary rejection | Pass |\n\n",
      )
      .replace(
        "Architecture Source**: N/A — governance-only example",
        "Architecture Source**: N/A — governance-only example\n- **Approver**: @linhai\n- **Approval Time**: 2026-08-13T00:00:00Z\n- **Approval Evidence**: Approve",
      ),
  );
  const indexPath = join(root, "docs/adr/INDEX.md");
  writeFileSync(
    indexPath,
    readFileSync(indexPath, "utf8")
      .replace("| Proposed |", "| Accepted |")
      .replace("| Not Started |", "| Complete |"),
  );

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /Risk Coverage Matrix dimension concurrency and ordering must be Pass or N\/A/i);
});

test("rejects a terminal risk row whose Pass evidence is a placeholder", () => {
  const root = validRepository();
  const path = join(root, "docs/adr/ADR-0001-example.md");
  writeFileSync(
    path,
    readFileSync(path, "utf8")
      .replace("Decision Status**: Proposed", "Decision Status**: Accepted")
      .replace("Implementation Status**: Not Started", "Implementation Status**: Complete")
      .replace(
        /## Risk Coverage Matrix \[Required\][\s\S]*?(?=## Acceptance Checks)/,
        "## Risk Coverage Matrix [Required]\n| Risk dimension | Status and stable evidence |\n| --- | --- |\n| concurrency and ordering | Pass — Pending |\n| timeout and deadline | Pass — real evidence |\n| cancellation and interruption | Pass — real evidence |\n| resource bounds and backpressure | Pass — real evidence |\n| framework or trust-boundary rejection | Pass — real evidence |\n\n",
      )
      .replace(
        "Architecture Source**: N/A — governance-only example",
        "Architecture Source**: N/A — governance-only example\n- **Approver**: @linhai\n- **Approval Time**: 2026-08-13T00:00:00Z\n- **Approval Evidence**: Approve",
      ),
  );
  const indexPath = join(root, "docs/adr/INDEX.md");
  writeFileSync(
    indexPath,
    readFileSync(indexPath, "utf8")
      .replace("| Proposed |", "| Accepted |")
      .replace("| Not Started |", "| Complete |"),
  );

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /Risk Coverage Matrix dimension concurrency and ordering Pass row must record stable evidence/i);
});

test("rejects a Completion Checklist with too few items", () => {
  const root = validRepository();
  const path = join(root, "docs/adr/ADR-0001-example.md");
  writeFileSync(
    path,
    readFileSync(path, "utf8")
      .replace("Decision Status**: Proposed", "Decision Status**: Accepted")
      .replace("Implementation Status**: Not Started", "Implementation Status**: Complete")
      .replace(
        "## Completion Checklist [Required]\nChecklist.",
        "## Completion Checklist [Required]\n| ID | Item | Status | Actual Evidence |\n| --- | --- | --- | --- |\n| A-1 | approved | Complete | done |",
      )
      .replace(
        "Architecture Source**: N/A — governance-only example",
        "Architecture Source**: N/A — governance-only example\n- **Approver**: @linhai\n- **Approval Time**: 2026-08-13T00:00:00Z\n- **Approval Evidence**: Approve",
      ),
  );
  const indexPath = join(root, "docs/adr/INDEX.md");
  writeFileSync(
    indexPath,
    readFileSync(indexPath, "utf8")
      .replace("| Proposed |", "| Accepted |")
      .replace("| Not Started |", "| Complete |"),
  );

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /Completion Checklist item A-2 is missing for a terminal record/i);
});

test("rejects a Completion Checklist whose A-1 is N/A", () => {
  const root = validRepository();
  const path = join(root, "docs/adr/ADR-0001-example.md");
  writeFileSync(
    path,
    readFileSync(path, "utf8")
      .replace("Decision Status**: Proposed", "Decision Status**: Accepted")
      .replace("Implementation Status**: Not Started", "Implementation Status**: Complete")
      .replace(
        "## Completion Checklist [Required]\nChecklist.",
        "## Completion Checklist [Required]\n| ID | Item | Status | Actual Evidence |\n| --- | --- | --- | --- |\n| A-1 | approved | N/A — bogus exemption | Pending |\n| A-2 | evidence | Complete | done |\n| A-3 | traceability | Complete | done |\n| A-4 | risk | Complete | done |\n| A-5 | scope | Complete | done |",
      )
      .replace(
        "Architecture Source**: N/A — governance-only example",
        "Architecture Source**: N/A — governance-only example\n- **Approver**: @linhai\n- **Approval Time**: 2026-08-13T00:00:00Z\n- **Approval Evidence**: Approve",
      ),
  );
  const indexPath = join(root, "docs/adr/INDEX.md");
  writeFileSync(
    indexPath,
    readFileSync(indexPath, "utf8")
      .replace("| Proposed |", "| Accepted |")
      .replace("| Not Started |", "| Complete |"),
  );

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /Completion Checklist item A-1 must be Complete/i);
});

test("rejects an OCR runbook whose Execute stage is missing", () => {
  const root = validRepository();
  completeOcr(root, "0005");
  const ocrPath = join(root, "docs/adr/ocr/OCR-0005-example.md");
  writeFileSync(
    ocrPath,
    readFileSync(ocrPath, "utf8").replace("### Execute [Required]", "### Preflight [Required]"),
  );

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /Core Runbook And Evidence is missing required stage Execute/i);
});

test("rejects a terminal OCR whose Closure fields are Pending", () => {
  const root = validRepository();
  completeOcr(root, "0006");
  const ocrPath = join(root, "docs/adr/ocr/OCR-0006-example.md");
  writeFileSync(
    ocrPath,
    readFileSync(ocrPath, "utf8")
      .replace("**Final result**: Completed and not promoted.", "**Final result**: Pending — not yet recorded.")
      .replace("**Authorization review**: Pass", "**Authorization review**: Pending"),
  );

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /Closure field Final result must record a complete terminal result/i);
});

test("rejects an Accepted ADR whose Decision body is Pending", () => {
  const root = validRepository();
  const path = join(root, "docs/adr/ADR-0001-example.md");
  writeFileSync(
    path,
    readFileSync(path, "utf8")
      .replace("Decision Status**: Proposed", "Decision Status**: Accepted")
      .replace("## Decision [Required]\nDecision.", "## Decision [Required]\nPending")
      .replace(
        "Architecture Source**: N/A — governance-only example",
        "Architecture Source**: N/A — governance-only example\n- **Approver**: @linhai\n- **Approval Time**: 2026-08-13T00:00:00Z\n- **Approval Evidence**: Approve",
      ),
  );
  const indexPath = join(root, "docs/adr/INDEX.md");
  writeFileSync(indexPath, readFileSync(indexPath, "utf8").replace("| Proposed |", "| Accepted |"));

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /Decision body must contain substantive content, not Pending/i);
});

test("rejects a terminal ADR missing checklist item A-6 through A-8", () => {
  const root = validRepository();
  const path = join(root, "docs/adr/ADR-0001-example.md");
  writeFileSync(
    path,
    readFileSync(path, "utf8")
      .replace("Decision Status**: Proposed", "Decision Status**: Accepted")
      .replace("Implementation Status**: Not Started", "Implementation Status**: Complete")
      .replace(
        "## Completion Checklist [Required]\nChecklist.",
        "## Completion Checklist [Required]\n| ID | Item | Status | Actual Evidence |\n| --- | --- | --- | --- |\n| A-1 | approved | Complete | done |\n| A-2 | evidence | Complete | done |\n| A-3 | traceability | Complete | done |\n| A-4 | risk | Complete | done |\n| A-5 | scope | Complete | done |",
      )
      .replace(
        "Architecture Source**: N/A — governance-only example",
        "Architecture Source**: N/A — governance-only example\n- **Approver**: @linhai\n- **Approval Time**: 2026-08-13T00:00:00Z\n- **Approval Evidence**: Approve",
      ),
  );
  const indexPath = join(root, "docs/adr/INDEX.md");
  writeFileSync(
    indexPath,
    readFileSync(indexPath, "utf8")
      .replace("| Proposed |", "| Accepted |")
      .replace("| Not Started |", "| Complete |"),
  );

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /Completion Checklist item A-6 is missing/i);
});

test("rejects an OCR Closure with Fail in a review field", () => {
  const root = validRepository();
  completeOcr(root, "0007");
  const ocrPath = join(root, "docs/adr/ocr/OCR-0007-example.md");
  writeFileSync(
    ocrPath,
    readFileSync(ocrPath, "utf8").replace(
      "**Authorization review**: Pass",
      "**Authorization review**: Fail — unauthorized attempt detected",
    ),
  );

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /Closure field Authorization review must be Pass/i);
});

test("rejects an Accepted OCR with Pending expected evidence", () => {
  const root = validRepository();
  acceptedOcr(root, "0008");
  const ocrPath = join(root, "docs/adr/ocr/OCR-0008-example.md");
  writeFileSync(
    ocrPath,
    readFileSync(ocrPath, "utf8").replace(
      "| T-1 | obj | scope | criterion | evidence | In Progress | Pending |",
      "| T-1 | obj | scope | criterion | Pending | In Progress | Pending |",
    ),
  );

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /Task Definition has an incomplete cell in a planned column for T-1/i);
});

test("rejects an Accepted OCR with an unchecked eligibility item", () => {
  const root = validRepository();
  acceptedOcr(root, "0009");
  const ocrPath = join(root, "docs/adr/ocr/OCR-0009-example.md");
  writeFileSync(
    ocrPath,
    readFileSync(ocrPath, "utf8").replace("- [x] Is reversible", "- [ ] Is reversible"),
  );

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /Eligibility item .*reversible.*must be present and confirmed/i);
});

test("requires Conditional Extensions in an Accepted OCR", () => {
  const root = validRepository();
  acceptedOcr(root, "0019");
  const path = join(root, "docs/adr/ocr/OCR-0019-example.md");
  writeFileSync(
    path,
    readFileSync(path, "utf8").replace(
      /## Conditional Extensions[^\n]*\n[\s\S]*?(?=## Closure)/,
      "",
    ),
  );

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /missing conditionally required section Conditional Extensions/i);
});

test("rejects Pending required content in a retired Complete OCR", () => {
  const root = validRepository();
  completeOcr(root, "0020");
  const activePath = join(root, "docs/adr/ocr/OCR-0020-example.md");
  const archivedRelative = "docs/adr/ocr/archive/OCR-0020-example.md";
  let content = readFileSync(activePath, "utf8")
    .replace("Decision Status**: Accepted", "Decision Status**: Deprecated")
    .replace(
      "Architecture Source**: N/A — example",
      "Architecture Source**: N/A — example\n- **Retired By**: @linhai\n- **Retirement Time**: 2026-08-14T00:00:00Z\n- **Retirement Evidence**: Deprecate\n- **Retirement Reason**: test retirement",
    );
  content = replaceSection(content, "Eligibility", "Pending");
  write(root, archivedRelative, content);
  unlinkSync(activePath);
  const indexPath = join(root, "docs/adr/INDEX.md");
  writeFileSync(
    indexPath,
    readFileSync(indexPath, "utf8")
      .replace("| OCR | OCR-0020 | Example | Accepted |", "| OCR | OCR-0020 | Example | Deprecated |")
      .replace("docs/adr/ocr/OCR-0020-example.md", archivedRelative),
  );

  const result = run(root);
  assert.equal(result.status, 1, result.stderr || result.stdout);
  assert.match(result.stderr, /Eligibility body must contain substantive content/i);
});

test("rejects an Accepted ADR with incomplete Acceptance Checks columns", () => {
  const root = validRepository();
  acceptedAdr(root, "0010");
  const adrPath = join(root, "docs/adr/ADR-0010-example.md");
  writeFileSync(
    adrPath,
    readFileSync(adrPath, "utf8").replace(
      /\| Check ID \| Subtask \| Binary acceptance point \| Preconditions or input \| Verification method \| Exact expected result \| Expected evidence \| Status \| Actual result and evidence \|\n\| --- \| --- \| --- \| --- \| --- \| --- \| --- \| --- \| --- \|\n\| AC-1 \| T-1 \| The validator rejects the invalid record\. \| Invalid fixture\. \| Run the validator\. \| Exit status is 1\. \| Validator output\. \| Not Started \| Pending \|/,
      "| Check ID | Subtask |\n| --- | --- |\n| AC-1 | T-1 |",
    ),
  );

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /Acceptance Checks table is missing required template columns/i);
});

test("rejects an Accepted ADR acceptance check that references an undeclared subtask", () => {
  const root = validRepository();
  acceptedAdr(root, "0011");
  const adrPath = join(root, "docs/adr/ADR-0011-example.md");
  writeFileSync(
    adrPath,
    readFileSync(adrPath, "utf8").replace("| AC-1 | T-1 |", "| AC-1 | T-999 |"),
  );

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /acceptance check AC-1 Subtask must reference a declared T-N ID/i);
});

test("rejects Contract-To-Check Traceability referencing an undeclared check", () => {
  const root = validRepository();
  acceptedAdr(root, "0012");
  const adrPath = join(root, "docs/adr/ADR-0012-example.md");
  writeFileSync(
    adrPath,
    readFileSync(adrPath, "utf8").replace("| TC-1 | ADR | The rule is enforced. | AC-1 |", "| TC-1 | ADR | The rule is enforced. | AC-999 |"),
  );

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /clause TC-1 must reference declared acceptance check AC-999/i);
});

test("rejects an Accepted ADR acceptance check with a Pending status", () => {
  const root = validRepository();
  acceptedAdr(root, "0013");
  const adrPath = join(root, "docs/adr/ADR-0013-example.md");
  const content = replaceSection(
    readFileSync(adrPath, "utf8"),
    "Acceptance Checks",
    "| Check ID | Subtask | Binary acceptance point | Preconditions or input | Verification method | Exact expected result | Expected evidence | Status | Actual result and evidence |\n| --- | --- | --- | --- | --- | --- | --- | --- | --- |\n| AC-1 | T-1 | The validator rejects the invalid record. | Invalid fixture. | Run the validator. | Exit status is 1. | Validator output. | Pending | Pending |",
  );
  writeFileSync(adrPath, content);

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /acceptance check AC-1 has an illegal Status \(Pending\)/i);
});

test("rejects an Accepted ADR traceability table missing template columns", () => {
  const root = validRepository();
  acceptedAdr(root, "0014");
  const adrPath = join(root, "docs/adr/ADR-0014-example.md");
  writeFileSync(
    adrPath,
    replaceSection(
      readFileSync(adrPath, "utf8"),
      "Contract-To-Check Traceability",
      "| Clause ID | Acceptance check or deterministic test IDs |\n| --- | --- |\n| TC-1 | AC-1 |",
    ),
  );

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /Contract-To-Check Traceability table is missing required template columns/i);
});

test("rejects an Accepted ADR risk matrix missing approval-gate columns", () => {
  const root = validRepository();
  acceptedAdr(root, "0015");
  const adrPath = join(root, "docs/adr/ADR-0015-example.md");
  writeFileSync(
    adrPath,
    replaceSection(
      readFileSync(adrPath, "utf8"),
      "Risk Coverage Matrix",
      RISK_MATRIX_TABLE,
    ),
  );

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /Risk Coverage Matrix table is missing required template columns/i);
});

test("rejects an Accepted OCR with terse eligibility keywords", () => {
  const root = validRepository();
  acceptedOcr(root, "0016");
  const ocrPath = join(root, "docs/adr/ocr/OCR-0016-example.md");
  writeFileSync(
    ocrPath,
    replaceSection(
      readFileSync(ocrPath, "utf8"),
      "Eligibility",
      "- [x] accepted data\n- [x] reversible\n- [x] Does not modify docs\n- [x] preflight\n- [x] no secret\n- [x] automatic",
    ),
  );

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /Eligibility item .*boundary.*must be present and confirmed/i);
});

test("rejects invalid Mermaid in a non-authoritative translation", () => {
  const root = validRepository();
  write(
    root,
    "docs/architecture/translations/zh-CN/ADD-0001-example.md",
    "```mermaid\nsequenceDiagram\n  A-->>B: invalid; message\n```\n",
  );

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /translations.*invalid Mermaid/i);
});

test("requires a terminal ADR using legacy traceability to retain risk-matrix fields", () => {
  const root = validRepository();
  acceptedAdr(root, "0017");
  const path = join(root, "docs/adr/ADR-0017-example.md");
  let content = readFileSync(path, "utf8")
    .replace("Implementation Status**: In Progress", "Implementation Status**: Complete");
  content = replaceSection(
    content,
    "Contract-To-Check Traceability",
    `| Clause ID | Normative contract clause | Acceptance check or deterministic test |
| --- | --- | --- |
| TC-1 | The rule is enforced. | AC-1 |`,
  );
  content = replaceSection(
    content,
    "Risk Coverage Matrix",
    `| Risk dimension | Status and stable evidence |
| --- | --- |
| concurrency and ordering | Pass — evidence |
| timeout and deadline | Pass — evidence |
| cancellation and interruption | Pass — evidence |
| resource bounds and backpressure | Pass — evidence |
| framework or trust-boundary rejection | Pass — evidence |`,
  );
  writeFileSync(path, content);
  const index = join(root, "docs/adr/INDEX.md");
  writeFileSync(index, readFileSync(index, "utf8").replace(
    "| Full ADR | ADR-0017 | Example | Accepted | In Progress |",
    "| Full ADR | ADR-0017 | Example | Accepted | Complete |",
  ));

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /Risk Coverage Matrix table is missing required template columns/i);
});

test("rejects an OCR eligibility item that negates the required boundary", () => {
  const root = validRepository();
  acceptedOcr(root, "0018");
  const path = join(root, "docs/adr/ocr/OCR-0018-example.md");
  writeFileSync(path, readFileSync(path, "utf8").replace(
    "Uses an accepted architecture, pipeline, artifact contract, security boundary, and data boundary.",
    "No accepted architecture, pipeline, or artifact boundary exists.",
  ));

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /Eligibility item .*boundary.*must be present and confirmed/i);
});

test("requires Stable Implementation Touchpoints for an Accepted Lightweight ADR", () => {
  const root = validRepository();
  acceptedAdr(root, "0019");
  const path = join(root, "docs/adr/ADR-0019-example.md");
  let content = readFileSync(path, "utf8")
    .replace("# ADR-0019: Example", "# Lightweight ADR-0019: Example")
    .replace("## Scope [Required]\nScope.", "## Scope [Required]\nScope.\n## Lightweight Eligibility Check [Required]\nEligible.")
    .replace(
      /### Stable Implementation Touchpoints[^\n]*\n\| Path[\s\S]*?\| working tree before commit \|\n/,
      "",
    );
  writeFileSync(path, content);
  const index = join(root, "docs/adr/INDEX.md");
  writeFileSync(index, readFileSync(index, "utf8").replace(
    "| Full ADR | ADR-0019 | Example |",
    "| Lightweight ADR | ADR-0019 | Example |",
  ));

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /Stable Implementation Touchpoints requires a structured table/i);
});

test("requires Stable Implementation Touchpoints for an Accepted Full source ADR", () => {
  const root = validRepository();
  acceptedAdr(root, "0020");
  const path = join(root, "docs/adr/ADR-0020-example.md");
  writeFileSync(path, readFileSync(path, "utf8").replace(
    /### Stable Implementation Touchpoints[^\n]*\n\| Path[\s\S]*?\| working tree before commit \|\n/,
    "",
  ));

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /Stable Implementation Touchpoints requires a structured table/i);
});

test("rejects a whole-section N/A for Stable Implementation Touchpoints in an active Accepted Full ADR", () => {
  const root = validRepository();
  acceptedAdr(root, "0022");
  const path = join(root, "docs/adr/ADR-0022-example.md");
  writeFileSync(path, readFileSync(path, "utf8").replace(
    /### Stable Implementation Touchpoints[^\n]*\n\| Path[\s\S]*?\| working tree before commit \|\n/,
    "### Stable Implementation Touchpoints [Conditionally Required — source or configuration implementation]\nN/A — governance documentation only.\n",
  ));

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /Stable Implementation Touchpoints requires a structured table/i);
});

test("accepts a reasoned N\/A excerpt when a stable touchpoint symbol is present", () => {
  const root = validRepository();
  acceptedAdr(root, "0021");
  const path = join(root, "docs/adr/ADR-0021-example.md");
  let content = readFileSync(path, "utf8")
    .replace("# ADR-0021: Example", "# Lightweight ADR-0021: Example")
    .replace(
      "## Scope [Required]\nScope.",
      "## Scope [Required]\nScope.\n## Lightweight Eligibility Check [Required]\nEligible.",
    );
  writeFileSync(path, content);
  const index = join(root, "docs/adr/INDEX.md");
  writeFileSync(index, readFileSync(index, "utf8").replace(
    "| Full ADR | ADR-0021 | Example |",
    "| Lightweight ADR | ADR-0021 | Example |",
  ));

  const result = run(root);
  assert.equal(result.status, 0, result.stderr || result.stdout);
});
