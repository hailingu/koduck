import assert from "node:assert/strict";
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import test from "node:test";

import { acceptedAdr, acceptedOcr, run, validRepository } from "./validate.test.mjs";

// Exercises lifecycle and structured-table boundaries that must not accept
// lookalike content outside the authoritative section or Markdown grammar.
function rewrite(root, path, transform) {
  const absolute = join(root, path);
  writeFileSync(absolute, transform(readFileSync(absolute, "utf8")));
}

function makeCurrentAdd(root) {
  rewrite(root, "docs/architecture/ADD-0001-example.md", (content) => content.replace(
    "- **Design Status**: Draft",
    `- **Design Status**: Current
- **Approver**: @linhai
- **Approval Time**: 2026-08-13T00:00:00Z
- **Approval Evidence**: Approve`,
  ));
  rewrite(root, "docs/architecture/INDEX.md", (content) => content.replace(
    "| ADD-0001 | Example | Draft |",
    "| ADD-0001 | Example | Current |",
  ));
}

test("does not satisfy Accepted approval metadata from Change Log history", () => {
  const root = validRepository();
  acceptedAdr(root, "0099");
  rewrite(root, "docs/adr/ADR-0099-example.md", (content) => content
    .replace(/- \*\*(?:Approver|Approval Time|Approval Evidence)\*\*:[^\n]+\n/g, "")
    .replace(
      "## Change Log [Required]\nInitial.",
      `## Change Log [Required]
Initial.
- **Approver**: @linhai
- **Approval Time**: 2026-08-13T00:00:00Z
- **Approval Evidence**: Approve`,
    ));

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /Accepted requires complete Approver.*Approval Time.*Approval Evidence/i);
});

test("rejects an Accepted OCR with a missing required runbook stage", () => {
  const root = validRepository();
  acceptedOcr(root, "0098");
  rewrite(root, "docs/adr/ocr/OCR-0098-example.md", (content) => content.replace(
    "### Execute [Required]",
    "### Other [Required]",
  ));

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /Core Runbook.*missing required stage Execute/i);
});

for (const [id, field] of [
  ["0097", "Operation Type"],
  ["0095", "Target Scope / Operation Owner"],
  ["0094", "Input Source or Version"],
  ["0093", "Expected Output or Target State"],
]) {
  test(`rejects an Accepted OCR with missing ${field} metadata`, () => {
    const root = validRepository();
    acceptedOcr(root, id);
    const escaped = field.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
    rewrite(root, `docs/adr/ocr/OCR-${id}-example.md`, (content) => content.replace(
      new RegExp(`^- \\*\\*${escaped}\\*\\*:[^\\n]+\\n`, "m"),
      "",
    ));

    const result = run(root);
    assert.equal(result.status, 1);
    assert.match(result.stderr, new RegExp(`missing required metadata field ${escaped}`, "i"));
  });
}

test("rejects an Accepted OCR with a missing stage actual-result field", () => {
  const root = validRepository();
  acceptedOcr(root, "0096");
  rewrite(root, "docs/adr/ocr/OCR-0096-example.md", (content) => content.replace(
    /^\*\*Actual result and stable evidence\*\*:[^\n]*\n/gm,
    "",
  ));

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /field Actual result and stable evidence must be present/i);
});

test("rejects a Current ADD component list without a Markdown table separator", () => {
  const root = validRepository();
  makeCurrentAdd(root);
  rewrite(root, "docs/architecture/ADD-0001-example.md", (content) => content.replace(
    "| --- | --- |\n| C-1 | Component |",
    "| C-1 | Component |",
  ));

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /Architecture Design requires a structured component table/i);
});

for (const [section, id, diagram] of [
  ["Control Flow Design", "CF-1", "flowchart LR\n  F[\"CF-1\"]"],
  ["Interaction Flow Design", "IX-1", "sequenceDiagram\n  participant U as IX-1"],
]) {
  test(`rejects a Current ADD ${section} list without a Markdown table separator`, () => {
    const root = validRepository();
    makeCurrentAdd(root);
    rewrite(root, "docs/architecture/ADD-0001-example.md", (content) => content.replace(
      new RegExp(`## ${section} \\[Conditionally Required — [^\\]]+\\]\\n[^\\n]+`),
      `## ${section} [Conditionally Required — triggered]
| ID | Description |
| ${id} | Flow |

\`\`\`mermaid
${diagram}
\`\`\``,
    ));

    const result = run(root);
    assert.equal(result.status, 1);
    assert.match(result.stderr, new RegExp(`${section} requires a structured table`, "i"));
  });
}
