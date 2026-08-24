// ADR: docs/adr/ADR-0002-required-ai-ci-postgres-verification.md

import assert from "node:assert/strict";
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import test from "node:test";
import { ACCEPTED_RISK_MATRIX_TABLE, RISK_MATRIX_TABLE, acceptedAdr, acceptedOcr, completeOcr, replaceSection, run, validRepository, write } from "./validate.test.mjs";
test("accepts a structurally valid governance repository", () => {
  const result = run(validRepository());
  assert.equal(result.status, 0, result.stderr || result.stdout);
});

test("rejects a record missing a required section", () => {
  const root = validRepository();
  const path = join(root, "docs/adr/ADR-0001-example.md");
  const content = readFileSync(path, "utf8").replace("## Decision [Required]\n", "");
  writeFileSync(path, content);

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /missing required section.*Decision/i);
});

test("rejects a duplicated normalized required section", () => {
  const root = validRepository();
  const path = join(root, "docs/adr/ADR-0001-example.md");
  const content = `${readFileSync(path, "utf8")}\n## Decision [Required]\nContradictory duplicate.\n`;
  writeFileSync(path, content);

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /duplicate required section.*Decision/i);
});

test("rejects a required section without a requirement-level label", () => {
  const root = validRepository();
  const path = join(root, "docs/adr/ADR-0001-example.md");
  writeFileSync(path, readFileSync(path, "utf8").replace("## Decision [Required]", "## Decision"));

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /section Decision must declare a requirement level/i);
});

test("rejects an illegal lifecycle status", () => {
  const root = validRepository();
  const path = join(root, "docs/adr/ADR-0001-example.md");
  const content = readFileSync(path, "utf8").replace(
    "Decision Status**: Proposed",
    "Decision Status**: Banana",
  );
  writeFileSync(path, content);

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /illegal Decision Status.*Banana/i);
});

test("rejects a dangling index path", () => {
  const root = validRepository();
  const path = join(root, "docs/adr/INDEX.md");
  const content = readFileSync(path, "utf8").replace(
    "docs/adr/ADR-0001-example.md",
    "docs/adr/ADR-9999-missing.md",
  );
  writeFileSync(path, content);

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /index path does not exist.*ADR-9999-missing/i);
});

test("rejects a stale ADD to ADR reciprocal link", () => {
  const root = validRepository();
  const addPath = join(root, "docs/architecture/ADD-0001-example.md");
  const content = readFileSync(addPath, "utf8").replace(
    "| CAND-1 | Ready | None |",
    "| CAND-1 | Selected | docs/adr/ADR-0001-example.md |",
  );
  writeFileSync(addPath, content);

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /reciprocal.*Architecture Source/i);
});

test("rejects a Selected ADD candidate whose ADR path is missing", () => {
  const root = validRepository();
  const addPath = join(root, "docs/architecture/ADD-0001-example.md");
  writeFileSync(
    addPath,
    readFileSync(addPath, "utf8").replace(
      "| CAND-1 | Ready | None |",
      "| CAND-1 | Selected | docs/adr/ADR-9999-missing.md |",
    ),
  );

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /linked ADR path does not exist.*ADR-9999-missing/i);
});

test("rejects an ADR whose Architecture Source has no reciprocal ADD link", () => {
  const root = validRepository();
  const adrPath = join(root, "docs/adr/ADR-0001-example.md");
  writeFileSync(
    adrPath,
    readFileSync(adrPath, "utf8").replace(
      "Architecture Source**: N/A — governance-only example",
      "Architecture Source**: docs/architecture/ADD-0001-example.md — CAND-1",
    ),
  );

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /reciprocal ADD candidate link is missing/i);
});

test("rejects invalid Mermaid syntax", () => {
  const root = validRepository();
  const path = join(root, "docs/architecture/ADD-0001-example.md");
  const content = readFileSync(path, "utf8").replace("flowchart LR", "flowchart ???");
  writeFileSync(path, content);

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /invalid Mermaid/i);
});

test("rejects unresolved template variables in instantiated records", () => {
  const root = validRepository();
  const path = join(root, "docs/adr/ADR-0001-example.md");
  writeFileSync(path, readFileSync(path, "utf8").replace("Context.", "{{CONTEXT}}"));

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /unresolved template variable.*CONTEXT/i);
});

test("rejects Current ADD content without complete approval metadata", () => {
  const root = validRepository();
  const addPath = join(root, "docs/architecture/ADD-0001-example.md");
  writeFileSync(
    addPath,
    readFileSync(addPath, "utf8").replace("Design Status**: Draft", "Design Status**: Current"),
  );
  const indexPath = join(root, "docs/architecture/INDEX.md");
  writeFileSync(indexPath, readFileSync(indexPath, "utf8").replace("| Draft |", "| Current |"));

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /Current requires complete Approver.*Approval Time.*Approval Evidence/i);
});

test("rejects a Mermaid architecture diagram that omits a component table ID", () => {
  const root = validRepository();
  const path = join(root, "docs/architecture/ADD-0001-example.md");
  writeFileSync(path, readFileSync(path, "utf8").replace('C1["C-1"]', 'C1["Component"]'));

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /Mermaid architecture diagram does not cover.*C-1/i);
});

test("rejects a Mermaid ID prefix that omits the complete component ID", () => {
  const root = validRepository();
  const path = join(root, "docs/architecture/ADD-0001-example.md");
  writeFileSync(path, readFileSync(path, "utf8").replace('C1["C-1"]', 'C10["C-10"]'));

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /Mermaid architecture diagram does not cover.*C-1/i);
});

test("rejects index status that disagrees with the indexed record", () => {
  const root = validRepository();
  const path = join(root, "docs/adr/INDEX.md");
  writeFileSync(path, readFileSync(path, "utf8").replace("| Proposed |", "| Accepted |"));

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /index Decision Status Accepted disagrees.*Proposed/i);
});

test("rejects a decision record omitted from the repository index", () => {
  const root = validRepository();
  write(
    root,
    "docs/adr/ADR-0002-unindexed.md",
    readFileSync(join(root, "docs/adr/ADR-0001-example.md"), "utf8").replace(
      "# ADR-0001: Example",
      "# ADR-0002: Unindexed example",
    ),
  );

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /ADR-0002-unindexed\.md: record is missing from docs\/adr\/INDEX\.md/i);
});

test("rejects an undeclared template variable", () => {
  const root = validRepository();
  write(root, "docs/adr/template/0000-template.md", "# {{UNDECLARED}}\n");

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /template variable UNDECLARED is not declared/i);
});

test("ignores pseudo-headings inside fenced code blocks", () => {
  const root = validRepository();
  const path = join(root, "docs/adr/ADR-0001-example.md");
  writeFileSync(
    path,
    readFileSync(path, "utf8").replace(
      "## Context [Required]\nContext.",
      "## Context [Required]\nContext.\n\n```\n## Example\nthis is code, not a section\n```\n",
    ),
  );

  const result = run(root);
  assert.equal(result.status, 0, result.stderr || result.stdout);
});

test("does not satisfy the Decision section with Decision Drivers", () => {
  const root = validRepository();
  const path = join(root, "docs/adr/ADR-0001-example.md");
  // The fixture keeps `Decision Drivers` present; removing `Decision` must
  // still fail because an exact name, not a prefix, is required.
  writeFileSync(
    path,
    readFileSync(path, "utf8").replace("## Decision [Required]\nDecision.\n", ""),
  );

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /missing required section Decision/i);
});

test("rejects a Full ADR missing a required Options Considered section", () => {
  const root = validRepository();
  const path = join(root, "docs/adr/ADR-0001-example.md");
  writeFileSync(
    path,
    readFileSync(path, "utf8").replace("## Options Considered [Required]\nOptions.\n", ""),
  );

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /missing required section Options Considered/i);
});

test("rejects an ADD missing a required Cross-Cutting Design section", () => {
  const root = validRepository();
  const path = join(root, "docs/architecture/ADD-0001-example.md");
  writeFileSync(
    path,
    readFileSync(path, "utf8").replace(
      "## Cross-Cutting Design [Required]\nCross-cutting concerns.\n",
      "",
    ),
  );

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /missing required section Cross-Cutting Design/i);
});

test("rejects a Lightweight ADR missing its required eligibility check", () => {
  const root = validRepository();
  write(
    root,
    "docs/adr/ADR-0002-lightweight.md",
    `# Lightweight ADR-0002: Example

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
## Decision [Required]
Decision.
## Scope [Required]
Scope.
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
  const indexPath = join(root, "docs/adr/INDEX.md");
  writeFileSync(
    indexPath,
    readFileSync(indexPath, "utf8").concat(
      "| Lightweight ADR | ADR-0002 | Example | Proposed | Not Started | Project | N/A — governance-only example | docs/adr/ADR-0002-lightweight.md | None |\n",
    ),
  );

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /missing required section Lightweight Eligibility Check/i);
});

test("accepts a complete Lightweight ADR", () => {
  const root = validRepository();
  write(
    root,
    "docs/adr/ADR-0002-lightweight.md",
    `# Lightweight ADR-0002: Example

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
## Decision [Required]
Decision.
## Scope [Required]
Scope.
## Lightweight Eligibility Check [Required]
Eligibility.
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
  const indexPath = join(root, "docs/adr/INDEX.md");
  writeFileSync(
    indexPath,
    readFileSync(indexPath, "utf8").concat(
      "| Lightweight ADR | ADR-0002 | Example | Proposed | Not Started | Project | N/A — governance-only example | docs/adr/ADR-0002-lightweight.md | None |\n",
    ),
  );

  const result = run(root);
  assert.equal(result.status, 0, result.stderr || result.stdout);
});

test("rejects an Architecture Source that is neither an ADD link nor N/A", () => {
  const root = validRepository();
  const path = join(root, "docs/adr/ADR-0001-example.md");
  writeFileSync(
    path,
    readFileSync(path, "utf8").replace(
      "Architecture Source**: N/A — governance-only example",
      "Architecture Source**: docs/some-other-record.md",
    ),
  );

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /Architecture Source must be an ADD path plus candidate ID or N\/A/i);
});

test("rejects a missing Architecture Source field", () => {
  const root = validRepository();
  const path = join(root, "docs/adr/ADR-0001-example.md");
  writeFileSync(
    path,
    readFileSync(path, "utf8").replace(
      "- **Architecture Source**: N/A — governance-only example\n",
      "",
    ),
  );

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /Architecture Source is missing/i);
});

test("rejects a reciprocal candidate ID matched only by substring", () => {
  const root = validRepository();
  const addPath = join(root, "docs/architecture/ADD-0001-example.md");
  writeFileSync(
    addPath,
    readFileSync(addPath, "utf8").replace(
      "| CAND-1 | Ready | None |",
      "| CAND-1 | Selected | docs/adr/ADR-0001-example.md |\n| CAND-10 | Selected | docs/adr/ADR-0001-example.md |",
    ),
  );
  const adrPath = join(root, "docs/adr/ADR-0001-example.md");
  writeFileSync(
    adrPath,
    readFileSync(adrPath, "utf8").replace(
      "Architecture Source**: N/A — governance-only example",
      "Architecture Source**: docs/architecture/ADD-0001-example.md — CAND-10",
    ),
  );

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /reciprocal Architecture Source is missing.*CAND-1/i);
});

test("rejects a Full ADR missing Contract-To-Check Traceability", () => {
  const root = validRepository();
  const path = join(root, "docs/adr/ADR-0001-example.md");
  writeFileSync(
    path,
    readFileSync(path, "utf8").replace(
      "## Contract-To-Check Traceability [Required]\nTraceability.\n",
      "",
    ),
  );

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /missing required section Contract-To-Check Traceability/i);
});

test("rejects a Full ADR missing its Risk Coverage Matrix", () => {
  const root = validRepository();
  const path = join(root, "docs/adr/ADR-0001-example.md");
  writeFileSync(path, readFileSync(path, "utf8").replace("## Risk Coverage Matrix [Required]\n", ""));

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /missing required section Risk Coverage Matrix/i);
});

test("rejects a Risk Coverage Matrix missing a baseline dimension", () => {
  const root = validRepository();
  const path = join(root, "docs/adr/ADR-0001-example.md");
  writeFileSync(
    path,
    readFileSync(path, "utf8").replace("| cancellation and interruption | covered |\n", ""),
  );

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(
    result.stderr,
    /Risk Coverage Matrix is missing baseline dimension cancellation and interruption/i,
  );
});

test("does not read metadata from inside fenced code blocks", () => {
  const root = validRepository();
  const path = join(root, "docs/adr/ADR-0001-example.md");
  writeFileSync(
    path,
    readFileSync(path, "utf8")
      .replace("- **Decision Status**: Proposed\n", "")
      .replace(
        "## Context [Required]\nContext.",
        "## Context [Required]\nContext.\n\n```\n- **Decision Status**: Proposed\n```",
      ),
  );

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /illegal Decision Status/i);
});

test("reads the ADR path only from the candidate table column", () => {
  const root = validRepository();
  const addPath = join(root, "docs/architecture/ADD-0001-example.md");
  // CAND-1 is Selected and its evidence column names the ADR path, but the
  // formal `ADR path` column is `None`; the validator must read the column.
  writeFileSync(
    addPath,
    readFileSync(addPath, "utf8")
      .replace(
        "| ID | Status | ADR path |",
        "| ID | Status | Status reason or evidence | ADR path |",
      )
      .replace("| --- | --- | --- |", "| --- | --- | --- | --- |")
      .replace(
        "| CAND-1 | Ready | None |",
        "| CAND-1 | Selected | selected via `docs/adr/ADR-0001-example.md` | None |",
      ),
  );
  const adrPath = join(root, "docs/adr/ADR-0001-example.md");
  writeFileSync(
    adrPath,
    readFileSync(adrPath, "utf8").replace(
      "Architecture Source**: N/A — governance-only example",
      "Architecture Source**: docs/architecture/ADD-0001-example.md — CAND-1",
    ),
  );

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /CAND-1 is missing its linked ADR path/i);
});

test("rejects a malformed ADR Task Candidates table instead of skipping validation", () => {
  const root = validRepository();
  const addPath = join(root, "docs/architecture/ADD-0001-example.md");
  // The header drops the `ADR path` column, so the table cannot be parsed; a
  // Selected candidate must not bypass the reciprocal-link check.
  writeFileSync(
    addPath,
    readFileSync(addPath, "utf8")
      .replace("| ID | Status | ADR path |", "| ID | Status |")
      .replace(
        "| CAND-1 | Ready | None |",
        "| CAND-1 | Selected | docs/adr/ADR-0001-example.md |",
      ),
  );

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /ADR Task Candidates table is missing required/i);
});

test("rejects a Risk Coverage Matrix with a duplicated dimension", () => {
  const root = validRepository();
  const path = join(root, "docs/adr/ADR-0001-example.md");
  writeFileSync(
    path,
    readFileSync(path, "utf8").replace(
      "| resource bounds and backpressure | covered |\n",
      "| resource bounds and backpressure | covered |\n| cancellation and interruption | duplicate |\n",
    ),
  );

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /duplicates baseline dimension cancellation and interruption/i);
});

test("rejects a Risk Coverage Matrix with a non-baseline dimension row", () => {
  const root = validRepository();
  const path = join(root, "docs/adr/ADR-0001-example.md");
  writeFileSync(
    path,
    readFileSync(path, "utf8").replace(
      "| resource bounds and backpressure | covered |\n",
      "| resource bounds and backpressure | covered |\n| some extra dimension | extra |\n",
    ),
  );

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /non-baseline dimension row/i);
});

test("rejects an ADD candidate with an illegal status", () => {
  const root = validRepository();
  const addPath = join(root, "docs/architecture/ADD-0001-example.md");
  writeFileSync(
    addPath,
    readFileSync(addPath, "utf8").replace("| CAND-1 | Ready | None |", "| CAND-1 | Banana | None |"),
  );

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /CAND-1 has illegal status Banana/i);
});

test("rejects a Selected candidate whose linked ADR is already terminal", () => {
  const root = validRepository();
  const addPath = join(root, "docs/architecture/ADD-0001-example.md");
  writeFileSync(
    addPath,
    readFileSync(addPath, "utf8").replace(
      "| CAND-1 | Ready | None |",
      "| CAND-1 | Selected | docs/adr/ADR-0001-example.md |",
    ),
  );
  const adrPath = join(root, "docs/adr/ADR-0001-example.md");
  writeFileSync(
    adrPath,
    readFileSync(adrPath, "utf8")
      .replace(
        "Architecture Source**: N/A — governance-only example",
        "Architecture Source**: docs/architecture/ADD-0001-example.md — CAND-1",
      )
      .replace("Implementation Status**: Not Started", "Implementation Status**: Complete"),
  );
  const indexPath = join(root, "docs/adr/INDEX.md");
  writeFileSync(indexPath, readFileSync(indexPath, "utf8").replace("| Not Started |", "| Complete |"));

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /CAND-1 is Selected but linked ADR/i);
});

test("rejects a Complete candidate whose linked ADR is not terminal", () => {
  const root = validRepository();
  const addPath = join(root, "docs/architecture/ADD-0001-example.md");
  writeFileSync(
    addPath,
    readFileSync(addPath, "utf8").replace(
      "| CAND-1 | Ready | None |",
      "| CAND-1 | Complete | docs/adr/ADR-0001-example.md |",
    ),
  );
  const adrPath = join(root, "docs/adr/ADR-0001-example.md");
  writeFileSync(
    adrPath,
    readFileSync(adrPath, "utf8").replace(
      "Architecture Source**: N/A — governance-only example",
      "Architecture Source**: docs/architecture/ADD-0001-example.md — CAND-1",
    ),
  );

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /CAND-1 is Complete but linked ADR/i);
});

test("rejects a Rejected ADR without rejection evidence", () => {
  const root = validRepository();
  const path = join(root, "docs/adr/ADR-0001-example.md");
  writeFileSync(
    path,
    readFileSync(path, "utf8")
      .replace("Decision Status**: Proposed", "Decision Status**: Rejected")
      .replace("Implementation Status**: Not Started", "Implementation Status**: Not Applicable"),
  );
  const indexPath = join(root, "docs/adr/INDEX.md");
  writeFileSync(
    indexPath,
    readFileSync(indexPath, "utf8")
      .replace("| Proposed |", "| Rejected |")
      .replace("| Not Started |", "| Not Applicable |"),
  );

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /Rejected requires complete Rejector.*Rejection Time.*Rejection Evidence/i);
});

test("rejects a retired ADR without retirement evidence", () => {
  const root = validRepository();
  const path = join(root, "docs/adr/ADR-0001-example.md");
  writeFileSync(
    path,
    readFileSync(path, "utf8")
      .replace("Decision Status**: Proposed", "Decision Status**: Deprecated")
      .replace("Implementation Status**: Not Started", "Implementation Status**: Not Applicable"),
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
  assert.match(
    result.stderr,
    /retired decision requires complete Retired By.*Retirement Time.*Retirement Evidence.*Retirement Reason/i,
  );
});

test("rejects an empty Risk Coverage Matrix without an explicit N/A", () => {
  const root = validRepository();
  const path = join(root, "docs/adr/ADR-0001-example.md");
  writeFileSync(
    path,
    readFileSync(path, "utf8").replace(
      /## Risk Coverage Matrix \[Required\][\s\S]*?(?=## Acceptance Checks)/,
      "## Risk Coverage Matrix [Required]\nNo coverage yet.\n\n",
    ),
  );

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /Risk Coverage Matrix must contain a five-dimension table or N\/A/i);
});

test("accepts an explicit N/A Risk Coverage Matrix with a reason", () => {
  const root = validRepository();
  const path = join(root, "docs/adr/ADR-0001-example.md");
  writeFileSync(
    path,
    readFileSync(path, "utf8").replace(
      /## Risk Coverage Matrix \[Required\][\s\S]*?(?=## Acceptance Checks)/,
      "## Risk Coverage Matrix [Required]\nN/A — governance-only ADR with no source implementation.\n\n",
    ),
  );

  const result = run(root);
  assert.equal(result.status, 0, result.stderr || result.stdout);
});

test("rejects a Deprecated ADD without retirement evidence", () => {
  const root = validRepository();
  const addPath = join(root, "docs/architecture/ADD-0001-example.md");
  writeFileSync(
    addPath,
    readFileSync(addPath, "utf8").replace("Design Status**: Draft", "Design Status**: Deprecated"),
  );
  const indexPath = join(root, "docs/architecture/INDEX.md");
  writeFileSync(indexPath, readFileSync(indexPath, "utf8").replace("| Draft |", "| Deprecated |"));

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(
    result.stderr,
    /retired decision requires complete Retired By.*Retirement Time.*Retirement Evidence.*Retirement Reason/i,
  );
});

test("rejects a Deprecated ADR whose retirement evidence does not match", () => {
  const root = validRepository();
  const path = join(root, "docs/adr/ADR-0001-example.md");
  writeFileSync(
    path,
    readFileSync(path, "utf8")
      .replace("Decision Status**: Proposed", "Decision Status**: Deprecated")
      .replace(
        "Implementation Status**: Not Started",
        "Implementation Status**: Not Applicable\n- **Retired By**: @codex\n- **Retirement Time**: 2026-08-13T00:00:00Z\n- **Retirement Evidence**: Supersede\n- **Retirement Reason**: test",
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
  assert.match(result.stderr, /retired decision requires complete Retired By.*Retirement Evidence: Deprecate/i);
});

test("rejects duplicate ADD candidate IDs", () => {
  const root = validRepository();
  const addPath = join(root, "docs/architecture/ADD-0001-example.md");
  writeFileSync(
    addPath,
    readFileSync(addPath, "utf8").replace(
      "| CAND-1 | Ready | None |",
      "| CAND-1 | Selected | docs/adr/ADR-0001-example.md |\n| CAND-1 | Ready | None |",
    ),
  );

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /duplicate candidate ID CAND-1/i);
});

test("rejects a role label as an Approver", () => {
  const root = validRepository();
  const path = join(root, "docs/adr/ADR-0001-example.md");
  writeFileSync(
    path,
    readFileSync(path, "utf8")
      .replace("Decision Status**: Proposed", "Decision Status**: Accepted")
      .replace(
        "Architecture Source**: N/A — governance-only example",
        "Architecture Source**: N/A — governance-only example\n- **Approver**: @Human\n- **Approval Time**: 2026-08-13T00:00:00Z\n- **Approval Evidence**: Approve",
      ),
  );
  const indexPath = join(root, "docs/adr/INDEX.md");
  writeFileSync(indexPath, readFileSync(indexPath, "utf8").replace("| Proposed |", "| Accepted |"));

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /Accepted requires complete Approver.*Approval Time.*Approval Evidence/i);
});

test("ignores metadata inside tilde-fenced code blocks", () => {
  const root = validRepository();
  const path = join(root, "docs/adr/ADR-0001-example.md");
  writeFileSync(
    path,
    readFileSync(path, "utf8")
      .replace("- **Decision Status**: Proposed\n", "")
      .replace(
        "## Context [Required]\nContext.",
        "## Context [Required]\nContext.\n\n~~~\n- **Decision Status**: Proposed\n~~~",
      ),
  );

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /illegal Decision Status/i);
});

test("rejects a template placeholder as an Approver", () => {
  const root = validRepository();
  const path = join(root, "docs/adr/ADR-0001-example.md");
  writeFileSync(
    path,
    readFileSync(path, "utf8")
      .replace("Decision Status**: Proposed", "Decision Status**: Accepted")
      .replace(
        "Architecture Source**: N/A — governance-only example",
        "Architecture Source**: N/A — governance-only example\n- **Approver**: @{{ACTOR_ID}}\n- **Approval Time**: 2026-08-13T00:00:00Z\n- **Approval Evidence**: Approve",
      ),
  );
  const indexPath = join(root, "docs/adr/INDEX.md");
  writeFileSync(indexPath, readFileSync(indexPath, "utf8").replace("| Proposed |", "| Accepted |"));

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /Accepted requires complete Approver/i);
});

test("rejects a Blocked ADR without blocker evidence", () => {
  const root = validRepository();
  const path = join(root, "docs/adr/ADR-0001-example.md");
  writeFileSync(
    path,
    readFileSync(path, "utf8")
      .replace("Decision Status**: Proposed", "Decision Status**: Accepted")
      .replace("Implementation Status**: Not Started", "Implementation Status**: Blocked")
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
      .replace("| Not Started |", "| Blocked |"),
  );

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(
    result.stderr,
    /Blocked requires complete Blocked From.*Blocker And Evidence.*Blocker Owner.*Blocker Exit/i,
  );
});

test("rejects an ADD candidate row with an illegal ID", () => {
  const root = validRepository();
  const addPath = join(root, "docs/architecture/ADD-0001-example.md");
  writeFileSync(
    addPath,
    readFileSync(addPath, "utf8").replace(
      "| CAND-1 | Ready | None |",
      "| CAND-1 | Ready | None |\n| CAND-X | Selected | docs/adr/ADR-0001-example.md |",
    ),
  );

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /row with a missing or illegal ID \(CAND-X\)/i);
});

test("rejects an ADD whose Architecture Design flowchart is only in another section", () => {
  const root = validRepository();
  const addPath = join(root, "docs/architecture/ADD-0001-example.md");
  writeFileSync(
    addPath,
    readFileSync(addPath, "utf8")
      .replace("Design Status**: Draft", "Design Status**: Current")
      .replace(
        "## Metadata [Required]",
        "## Metadata [Required]\n- **Approver**: @linhai\n- **Approval Time**: 2026-08-13T00:00:00Z\n- **Approval Evidence**: Approve",
      )
      .replace(/```mermaid[\s\S]*?```/, "")
      .replace(
        "## Cross-Cutting Design [Required]",
        "## Cross-Cutting Design [Required]\n\n```mermaid\nflowchart LR\n  C1[\"C-1\"]\n```",
      ),
  );
  const indexPath = join(root, "docs/architecture/INDEX.md");
  writeFileSync(indexPath, readFileSync(indexPath, "utf8").replace("| Draft |", "| Current |"));

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /Architecture Design requires a Mermaid flowchart in this section/i);
});

test("rejects a retired ADD that remains in the active directory", () => {
  const root = validRepository();
  const addPath = join(root, "docs/architecture/ADD-0001-example.md");
  writeFileSync(
    addPath,
    readFileSync(addPath, "utf8")
      .replace("Design Status**: Draft", "Design Status**: Deprecated")
      .replace(
        "## Metadata [Required]",
        "## Metadata [Required]\n- **Retired By**: @codex\n- **Retirement Time**: 2026-08-13T00:00:00Z\n- **Retirement Evidence**: Deprecate\n- **Retirement Reason**: test",
      ),
  );
  const indexPath = join(root, "docs/architecture/INDEX.md");
  writeFileSync(indexPath, readFileSync(indexPath, "utf8").replace("| Draft |", "| Deprecated |"));

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /retired .* record must reside under an archive/i);
});

test("does not close a four-backtick fence with three backticks", () => {
  const root = validRepository();
  const path = join(root, "docs/adr/ADR-0001-example.md");
  writeFileSync(
    path,
    readFileSync(path, "utf8")
      .replace("- **Decision Status**: Proposed\n", "")
      .replace(
        "## Context [Required]\nContext.",
        "## Context [Required]\nContext.\n\n````\ncode\n```\n- **Decision Status**: Proposed\n````",
      ),
  );

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /illegal Decision Status/i);
});

test("rejects a Proposed ADR that has entered In Progress", () => {
  const root = validRepository();
  const path = join(root, "docs/adr/ADR-0001-example.md");
  writeFileSync(
    path,
    readFileSync(path, "utf8").replace(
      "Implementation Status**: Not Started",
      "Implementation Status**: In Progress",
    ),
  );
  const indexPath = join(root, "docs/adr/INDEX.md");
  writeFileSync(indexPath, readFileSync(indexPath, "utf8").replace("| Not Started |", "| In Progress |"));

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /Proposed requires Implementation Status Not Started/i);
});

test("rejects Pending as Blocked blocker evidence", () => {
  const root = validRepository();
  const path = join(root, "docs/adr/ADR-0001-example.md");
  writeFileSync(
    path,
    readFileSync(path, "utf8")
      .replace("Decision Status**: Proposed", "Decision Status**: Accepted")
      .replace("Implementation Status**: Not Started", "Implementation Status**: Blocked")
      .replace(
        "Architecture Source**: N/A — governance-only example",
        "Architecture Source**: N/A — governance-only example\n- **Approver**: @linhai\n- **Approval Time**: 2026-08-13T00:00:00Z\n- **Approval Evidence**: Approve\n- **Blocked From**: Not Started\n- **Blocker And Evidence**: Pending\n- **Blocker Owner**: @codex\n- **Blocker Exit Or Recheck Criterion**: retry when the upstream review lands",
      ),
  );
  const indexPath = join(root, "docs/adr/INDEX.md");
  writeFileSync(
    indexPath,
    readFileSync(indexPath, "utf8")
      .replace("| Proposed |", "| Accepted |")
      .replace("| Not Started |", "| Blocked |"),
  );

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(
    result.stderr,
    /Blocked requires complete Blocked From.*Blocker And Evidence.*Blocker Owner.*Blocker Exit/i,
  );
});

test("rejects a non-existent ISO approval time", () => {
  const root = validRepository();
  const path = join(root, "docs/adr/ADR-0001-example.md");
  writeFileSync(
    path,
    readFileSync(path, "utf8")
      .replace("Decision Status**: Proposed", "Decision Status**: Accepted")
      .replace(
        "Architecture Source**: N/A — governance-only example",
        "Architecture Source**: N/A — governance-only example\n- **Approver**: @linhai\n- **Approval Time**: 2026-99-99T99:99:99+99:99\n- **Approval Evidence**: Approve",
      ),
  );
  const indexPath = join(root, "docs/adr/INDEX.md");
  writeFileSync(indexPath, readFileSync(indexPath, "utf8").replace("| Proposed |", "| Accepted |"));

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /Accepted requires complete Approver.*Approval Time.*Approval Evidence/i);
});

test("rejects a retired ADD with a non-terminal candidate", () => {
  const root = validRepository();
  const addPath = join(root, "docs/architecture/ADD-0001-example.md");
  writeFileSync(
    addPath,
    readFileSync(addPath, "utf8")
      .replace("Design Status**: Draft", "Design Status**: Deprecated")
      .replace(
        "## Metadata [Required]",
        "## Metadata [Required]\n- **Retired By**: @codex\n- **Retirement Time**: 2026-08-13T00:00:00Z\n- **Retirement Evidence**: Deprecate\n- **Retirement Reason**: test",
      ),
  );
  const indexPath = join(root, "docs/architecture/INDEX.md");
  writeFileSync(indexPath, readFileSync(indexPath, "utf8").replace("| Draft |", "| Deprecated |"));

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /retired ADD candidate CAND-1 must be Deferred or Complete/i);
});

test("rejects a Rejected ADR that remains in the active directory", () => {
  const root = validRepository();
  const path = join(root, "docs/adr/ADR-0001-example.md");
  writeFileSync(
    path,
    readFileSync(path, "utf8")
      .replace("Decision Status**: Proposed", "Decision Status**: Rejected")
      .replace("Implementation Status**: Not Started", "Implementation Status**: Not Applicable")
      .replace(
        "Architecture Source**: N/A — governance-only example",
        "Architecture Source**: N/A — governance-only example\n- **Rejector**: @codex\n- **Rejection Time**: 2026-08-13T00:00:00Z\n- **Rejection Evidence**: Reject",
      ),
  );
  const indexPath = join(root, "docs/adr/INDEX.md");
  writeFileSync(
    indexPath,
    readFileSync(indexPath, "utf8")
      .replace("| Proposed |", "| Rejected |")
      .replace("| Not Started |", "| Not Applicable |"),
  );

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /Rejected\/Not Applicable record must reside under an archive/i);
});

test("rejects a Current ADD that skips Architecture Design via N/A", () => {
  const root = validRepository();
  const addPath = join(root, "docs/architecture/ADD-0001-example.md");
  writeFileSync(
    addPath,
    readFileSync(addPath, "utf8")
      .replace("Design Status**: Draft", "Design Status**: Current")
      .replace(
        "## Metadata [Required]",
        "## Metadata [Required]\n- **Approver**: @linhai\n- **Approval Time**: 2026-08-13T00:00:00Z\n- **Approval Evidence**: Approve",
      )
      .replace(
        /## Architecture Design \[Required\][\s\S]*?(?=## Cross-Cutting Design)/,
        "## Architecture Design [Required]\nN/A — no architecture diagram.\n\n",
      ),
  );
  const indexPath = join(root, "docs/architecture/INDEX.md");
  writeFileSync(indexPath, readFileSync(indexPath, "utf8").replace("| Draft |", "| Current |"));

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(
    result.stderr,
    /Architecture Design requires a structured component table|Architecture Design requires a Mermaid flowchart in this section/i,
  );
});

test("allows a Draft ADD with incomplete Architecture Design", () => {
  const root = validRepository();
  const addPath = join(root, "docs/architecture/ADD-0001-example.md");
  writeFileSync(
    addPath,
    readFileSync(addPath, "utf8").replace(
      /## Architecture Design \[Required\][\s\S]*?(?=## Cross-Cutting Design)/,
      "## Architecture Design [Required]\nPending.\n\n",
    ),
  );

  const result = run(root);
  assert.equal(result.status, 0, result.stderr || result.stdout);
});

test("does not force-archive a Blocked OCR that may be re-attempted", () => {
  const root = validRepository();
  write(
    root,
    "docs/adr/ocr/OCR-0002-example.md",
    `# OCR-0002-example: Example

## Metadata [Required]
- **Decision Status**: Accepted
- **Implementation Status**: Blocked
- **Author**: @codex
- **Decision Owner**: @linhai
- **Required Approver**: @linhai
- **Record Scope**: Project
- **Operation Type**: Existing Runbook
- **Target Scope / Operation Owner**: Local fixture / @linhai
- **Input Source or Version**: Test fixture revision
- **Expected Output or Target State**: Fixture operation remains blocked
- **Architecture Source**: N/A — example OCR
- **Superseded By**: None
- **Approver**: @linhai
- **Approval Time**: 2026-08-13T00:00:00Z
- **Approval Evidence**: Approve
- **Blocked From**: Not Started
- **Blocker And Evidence**: upstream migration pending; evidence: ticket-123
- **Blocker Owner**: @codex
- **Blocker Exit Or Recheck Criterion**: retry once migration-123 lands

## Requirement Level Legend [Required]
Complete.
## Task Definition [Required]
| ID | Objective | Included scope | Completion criterion | Expected evidence | Status | Actual evidence |
| --- | --- | --- | --- | --- | --- | --- |
| T-1 | obj | scope | criterion | evidence | Not Started | Pending |
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
      "| OCR | OCR-0002 | Example | Accepted | Blocked | Project | N/A — example OCR | docs/adr/ocr/OCR-0002-example.md | None |\n",
    ),
  );

  const result = run(root);
  assert.equal(result.status, 0, result.stderr || result.stdout);
});
