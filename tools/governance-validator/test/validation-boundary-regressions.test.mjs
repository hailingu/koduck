// ADR: docs/adr/ADR-0002-required-ai-ci-postgres-verification.md

import assert from "node:assert/strict";
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import test from "node:test";

import { acceptedAdr, acceptedOcr, run, validRepository } from "./fixtures.mjs";

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

for (const [id, field, canonical, contradictory] of [
  ["0080", "Decision Status", "Accepted", "Proposed"],
  ["0081", "Approver", "@linhai", "@codex"],
  ["0082", "Approval Evidence", "Approve", "Reject"],
]) {
  test(`rejects duplicate active ${field} metadata`, () => {
    const root = validRepository();
    acceptedAdr(root, id);
    rewrite(root, `docs/adr/ADR-${id}-example.md`, (content) => content.replace(
      `- **${field}**: ${canonical}`,
      `- **${field}**: ${canonical}\n- **${field}**: ${contradictory}`,
    ));

    const result = run(root);
    assert.equal(result.status, 1);
    assert.match(
      result.stderr,
      new RegExp(`metadata field ${field} must appear exactly once; found 2`, "i"),
    );
  });
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

test("does not satisfy Accepted approval metadata from an HTML comment", () => {
  const root = validRepository();
  acceptedAdr(root, "0091");
  rewrite(root, "docs/adr/ADR-0091-example.md", (content) => content
    .replace(/- \*\*(?:Approver|Approval Time|Approval Evidence)\*\*:[^\n]+\n/g, "")
    .replace(
      "## Requirement Level Legend [Required]",
      `<!--
- **Approver**: @linhai
- **Approval Time**: 2026-08-13T00:00:00Z
- **Approval Evidence**: Approve
-->

## Requirement Level Legend [Required]`,
    ));

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /Accepted requires complete Approver.*Approval Time.*Approval Evidence/i);
});

test("does not satisfy a required section from an HTML comment", () => {
  const root = validRepository();
  rewrite(root, "docs/adr/ADR-0001-example.md", (content) => content.replace(
    "## Decision [Required]\nDecision.",
    "<!--\n## Decision [Required]\nDecision.\n-->",
  ));

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /missing required section Decision/i);
});

test("does not satisfy a structured table from an HTML comment", () => {
  const root = validRepository();
  makeCurrentAdd(root);
  rewrite(root, "docs/architecture/ADD-0001-example.md", (content) => content.replace(
    `| ID | Component |
| --- | --- |
| C-1 | Component |`,
    `<!--
| ID | Component |
| --- | --- |
| C-1 | Component |
-->`,
  ));

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /Architecture Design requires a structured component table/i);
});

test("does not satisfy an eligibility checklist item from an HTML comment", () => {
  const root = validRepository();
  acceptedOcr(root, "0090");
  rewrite(root, "docs/adr/ocr/OCR-0090-example.md", (content) => content.replace(
    "- [x] Uses an accepted architecture, pipeline, artifact contract, security boundary, and data boundary.",
    `<!--
- [x] Uses an accepted architecture, pipeline, artifact contract, security boundary, and data boundary.
-->`,
  ));

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /Eligibility item "boundary" must be present and confirmed/i);
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

test("rejects a standalone Pending placeholder later in a conditional section", () => {
  const root = validRepository();
  makeCurrentAdd(root);
  rewrite(root, "docs/architecture/ADD-0001-example.md", (content) => content.replace(
    "## Assumptions And Open Questions [Conditionally Required — assumptions exist]\nN/A — this fixture has no assumptions or open questions.",
    `## Assumptions And Open Questions [Conditionally Required — assumptions exist]
Substantive opening sentence.

Pending — resolution pending
`,
  ));

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /Assumptions And Open Questions body must contain substantive content/i);
});

test("accepts a reasoned N/A in a retained optional section", () => {
  const root = validRepository();
  acceptedAdr(root, "0094");
  rewrite(root, "docs/adr/ADR-0094-example.md", (content) => content.replace(
    "## Change Log [Required]\nInitial.",
    `## Supporting Notes [Optional]
N/A — no supporting notes apply to this record

## Change Log [Required]\nInitial.`,
  ));

  const result = run(root);
  assert.equal(result.status, 0, `a reasoned N/A optional section passes, found ${result.stderr}`);
});

test("rejects a bare unreasoned N/A in a retained optional section", () => {
  const root = validRepository();
  acceptedAdr(root, "0095");
  rewrite(root, "docs/adr/ADR-0095-example.md", (content) => content.replace(
    "## Change Log [Required]\nInitial.",
    `## Supporting Notes [Optional — informative]
N/A

## Change Log [Required]\nInitial.`,
  ));

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /Supporting Notes.*retained optional section.*complete content/i);
});

test("rejects a Pending value inside a Markdown field", () => {
  const root = validRepository();
  acceptedAdr(root, "0093");
  rewrite(root, "docs/adr/ADR-0093-example.md", (content) => content.replace(
    "## Context [Required]\nContext.",
    `## Context [Required]
Substantive opening sentence.

- **Owner**: Pending
`,
  ));

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /Context body must contain substantive content/i);
});

test("rejects a Pending value inside a Markdown table cell", () => {
  const root = validRepository();
  acceptedAdr(root, "0092");
  rewrite(root, "docs/adr/ADR-0092-example.md", (content) => content.replace(
    "## Context [Required]\nContext.",
    `## Context [Required]
| Field | Value |
| --- | --- |
| Owner | Pending |
`,
  ));

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /Context body must contain substantive content/i);
});

test("rejects a standalone Pending placeholder later in a required section", () => {
  const root = validRepository();
  acceptedAdr(root, "0097");
  rewrite(root, "docs/adr/ADR-0097-example.md", (content) => content.replace(
    "## Context [Required]\nContext.",
    `## Context [Required]
Substantive opening sentence.

Pending — reapproval required
`,
  ));

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /Context body must contain substantive content/i);
});

test("rejects a blank retained optional section at a final lifecycle gate", () => {
  const root = validRepository();
  acceptedAdr(root, "0096");
  rewrite(root, "docs/adr/ADR-0096-example.md", (content) => content.replace(
    "## Change Log [Required]\nInitial.",
    `## Supporting Notes [Optional — informative]
Pending

## Change Log [Required]\nInitial.`,
  ));

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /Supporting Notes.*retained optional section.*complete content/i);
});

test("does not count a Mermaid comment as covering a flow ID", () => {
  const root = validRepository();
  makeCurrentAdd(root);
  rewrite(root, "docs/architecture/ADD-0001-example.md", (content) => content.replace(
    "## Control Flow Design [Conditionally Required — multi-step behavior]\nN/A — this fixture has no multi-step control flow.",
    `## Control Flow Design [Conditionally Required — triggered]
| ID | Description |
| --- | --- |
| CF-1 | Flow |

\`\`\`mermaid
flowchart LR
  F["Unrelated step"]
  %% CF-1
\`\`\``,
  ));

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /Mermaid control-flow diagram does not cover CF-1/i);
});

test("does not exempt a triggered flow section from its diagram when N/A appears after real content", () => {
  const root = validRepository();
  makeCurrentAdd(root);
  rewrite(root, "docs/architecture/ADD-0001-example.md", (content) => content.replace(
    "## Control Flow Design [Conditionally Required — multi-step behavior]\nN/A — this fixture has no multi-step control flow.",
    `## Control Flow Design [Conditionally Required — multi-step behavior]
| ID | Description |
| --- | --- |
| CF-1 | Flow |

N/A — stray exemption line after a real table with no diagram.`,
  ));

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /Control Flow Design requires a Mermaid diagram for a Current ADD/i);
});

test("does not count a Mermaid example nested inside an outer fence as the required diagram", () => {
  const root = validRepository();
  makeCurrentAdd(root);
  rewrite(root, "docs/architecture/ADD-0001-example.md", (content) => content.replace(
    "```mermaid\nflowchart LR\n  C1[\"C-1\"]\n```",
    "````text\nAn embedded example:\n\n```mermaid\nflowchart LR\n  C1[\"C-1\"]\n```\n````",
  ));

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /Architecture Design requires a Mermaid flowchart in this section/i);
});

test("does not syntax-check a Mermaid example nested inside an outer fence", () => {
  const root = validRepository();
  rewrite(root, "docs/architecture/ADD-0001-example.md", (content) => content.replace(
    "## Control Flow Design [Conditionally Required — multi-step behavior]",
    `## Control Flow Design [Conditionally Required — multi-step behavior]

\`\`\`\`text
\`\`\`mermaid
garbage diagram syntax ((( [[
\`\`\`
\`\`\`\`
`,
  ));

  const result = run(root);
  assert.equal(result.status, 0, `nested example must not be syntax-checked: ${result.stderr}`);
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
