// ADR: docs/adr/ADR-0002-required-ai-ci-postgres-verification.md

import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdirSync, readFileSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";
import { acceptedAdr, run, validRepository, write } from "./fixtures.mjs";

const validator = fileURLToPath(new URL("../validate.mjs", import.meta.url));

test("exits with usage guidance when --root is missing", () => {
  const result = spawnSync(process.execPath, [validator], { encoding: "utf8" });
  assert.equal(result.status, 2);
  assert.match(result.stderr, /usage: node validate\.mjs --root <repository-root>/);
});

test("rejects an illegal Implementation Status value", () => {
  const root = validRepository();
  const path = join(root, "docs/adr/ADR-0001-example.md");
  writeFileSync(
    path,
    readFileSync(path, "utf8").replace("Implementation Status**: Not Started", "Implementation Status**: Bogus"),
  );
  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /illegal Implementation Status Bogus/);
});

test("rejects a Rejected record that is not Not Applicable", () => {
  const root = validRepository();
  const path = join(root, "docs/adr/ADR-0001-example.md");
  writeFileSync(
    path,
    readFileSync(path, "utf8").replace("Decision Status**: Proposed", "Decision Status**: Rejected"),
  );
  const indexPath = join(root, "docs/adr/INDEX.md");
  writeFileSync(
    indexPath,
    readFileSync(indexPath, "utf8").replace("| Proposed |", "| Rejected |"),
  );
  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /Rejected requires Implementation Status Not Applicable/);
});

test("rejects a Deprecated record without a terminal Implementation Status", () => {
  const root = validRepository();
  const path = join(root, "docs/adr/ADR-0001-example.md");
  let markdown = readFileSync(path, "utf8");
  markdown = markdown
    .replace("Decision Status**: Proposed", "Decision Status**: Deprecated")
    .replace("Implementation Status**: Not Started", "Implementation Status**: In Progress")
    .replace(
      "Architecture Source**: N/A — governance-only example",
      "Architecture Source**: N/A — governance-only example\n- **Retired By**: @linhai\n- **Retirement Time**: 2026-08-13T00:00:00Z\n- **Retirement Evidence**: Deprecate\n- **Retirement Reason**: retired for a newer design",
    );
  writeFileSync(path, markdown);
  const indexPath = join(root, "docs/adr/INDEX.md");
  writeFileSync(
    indexPath,
    readFileSync(indexPath, "utf8")
      .replace("| Proposed |", "| Deprecated |")
      .replace("| Not Started |", "| In Progress |"),
  );
  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /retired decision requires a terminal Implementation Status/);
});

test("rejects an illegal ADD Design Status", () => {
  const root = validRepository();
  const path = join(root, "docs/architecture/ADD-0001-example.md");
  writeFileSync(
    path,
    readFileSync(path, "utf8").replace("Design Status**: Draft", "Design Status**: Draftish"),
  );
  const indexPath = join(root, "docs/architecture/INDEX.md");
  writeFileSync(
    indexPath,
    readFileSync(indexPath, "utf8").replace("| Draft |", "| Draftish |"),
  );
  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /illegal Design Status Draftish/);
});

test("recognizes metadata fields carrying a requirement-level suffix", () => {
  const root = validRepository();
  acceptedAdr(root, "0002");
  const path = join(root, "docs/adr/ADR-0002-example.md");
  writeFileSync(
    path,
    readFileSync(path, "utf8").replace(
      "- **Approver**: @linhai",
      "- **Approver [Conditionally Required — Decision Status is Accepted]**: @linhai",
    ),
  );
  const result = run(root);
  assert.equal(result.status, 0, result.stderr);
});

// Writes an archived Superseded ADR naming the supplied replacement path; the
// index row's Superseded By cell defaults to the same path (or the supplied
// drifted value).
function supersededAdr(root, supersededBy, indexedAs = supersededBy) {
  const template = readFileSync(join(root, "docs/adr/ADR-0001-example.md"), "utf8");
  const markdown = template
    .replace("Decision Status**: Proposed", "Decision Status**: Superseded")
    .replace("Implementation Status**: Not Started", "Implementation Status**: Not Applicable")
    .replace(
      "Architecture Source**: N/A — governance-only example",
      "Architecture Source**: N/A — governance-only example\n- **Retired By**: @linhai\n- **Retirement Time**: 2026-08-13T00:00:00Z\n- **Retirement Evidence**: Supersede\n- **Retirement Reason**: replaced by a newer decision",
    )
    .replace("- **Superseded By**: None", `- **Superseded By**: ${supersededBy}`);
  write(root, "docs/adr/archive/ADR-0003-old.md", markdown);
  const indexPath = join(root, "docs/adr/INDEX.md");
  writeFileSync(
    indexPath,
    readFileSync(indexPath, "utf8").concat(
      `| Full ADR | ADR-0003 | Example | Superseded | Not Applicable | Project | N/A — governance-only example | docs/adr/archive/ADR-0003-old.md | ${indexedAs} |\n`,
    ),
  );
}

test("rejects a Superseded record whose replacement is not Accepted", () => {
  const root = validRepository();
  supersededAdr(root, "docs/adr/ADR-0001-example.md");
  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /Superseded By replacement docs\/adr\/ADR-0001-example\.md must be Accepted \(is Proposed\)/);
});

test("rejects a Superseded record whose replacement does not reciprocate", () => {
  const root = validRepository();
  acceptedAdr(root, "0002");
  supersededAdr(root, "docs/adr/ADR-0002-example.md");
  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /does not reciprocally Supersede this record/);
});

test("rejects a Superseded record whose replacement is not indexed", () => {
  const root = validRepository();
  const unindexed = readFileSync(join(root, "docs/adr/ADR-0001-example.md"), "utf8");
  write(root, "docs/adr/ADR-0004-unindexed.md", unindexed);
  // The index row drifts to `None`, so the replacement path appears nowhere in
  // the index and the not-in-the-index gate must fire alongside the drift.
  supersededAdr(root, "docs/adr/ADR-0004-unindexed.md", "None");
  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /Superseded By replacement docs\/adr\/ADR-0004-unindexed\.md is not in the index/);
});

test("rejects a Superseded record naming a non-record replacement", () => {
  const root = validRepository();
  supersededAdr(root, "docs/adr/INDEX.md");
  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /Superseded By must name an ADR, ADD, or OCR record: docs\/adr\/INDEX\.md/);
});

test("reports unresolved template variables in instantiated documents", () => {
  const root = validRepository();
  write(root, "docs/notes.md", "# Notes\n\nThis draft still contains {{FOO}}.\n");
  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /docs\/notes\.md: unresolved template variable FOO/);
});

test("reports undeclared and unused variables in template documents", () => {
  const root = validRepository();
  write(
    root,
    "docs/adr/template/authoring-guide.md",
    [
      "# Authoring guide",
      "",
      "| Declared variable | Use |",
      "| --- | --- |",
      "| `{{USED}}` | everywhere |",
      "| `{{LAZY}}` | nowhere |",
      "",
      "Use {{USED}} and {{USED}} here, plus {{NODECL}}.",
      "",
    ].join("\n"),
  );
  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /template variable NODECL is not declared/);
  assert.match(result.stderr, /declared template variable LAZY is not used/);
});

test("reports an index path that resolves outside the repository root", () => {
  const root = validRepository();
  const outside = join(tmpdir(), "koduck-governance-outside.md");
  writeFileSync(outside, "# Outside\n");
  const link = join(root, "docs/adr/ADR-0009-escape.md");
  symlinkSync(outside, link);
  const indexPath = join(root, "docs/adr/INDEX.md");
  writeFileSync(
    indexPath,
    readFileSync(indexPath, "utf8").concat(
      "| Full ADR | ADR-0009 | Example | Proposed | Not Started | Project | N/A — example | docs/adr/ADR-0009-escape.md | None |\n",
    ),
  );
  const result = run(root);
  rmSync(outside, { force: true });
  assert.equal(result.status, 1);
  assert.match(result.stderr, /index path resolves outside the repository root: docs\/adr\/ADR-0009-escape\.md/);
});

test("rejects an ADD candidate table missing a required column", () => {
  const root = validRepository();
  const path = join(root, "docs/architecture/ADD-0001-example.md");
  writeFileSync(
    path,
    readFileSync(path, "utf8").replace("| ID | Status | ADR path |", "| ID | Candidate | ADR path |"),
  );
  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /ADR Task Candidates table is missing required ID, Status, or ADR path columns/);
});

test("reports an index Trello Source that disagrees with the record", () => {
  const root = validRepository();
  const indexPath = join(root, "docs/architecture/INDEX.md");
  writeFileSync(
    indexPath,
    readFileSync(indexPath, "utf8").replace(
      "https://example.test/card",
      "https://example.test/different-card",
    ),
  );
  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /index Trello Source disagrees with record for docs\/architecture\/ADD-0001-example\.md/);
});

// Makes the ADD fixture Current with complete approval metadata for tests of
// the Current-only diagram gates.
function currentAdd(root) {
  const path = join(root, "docs/architecture/ADD-0001-example.md");
  let markdown = readFileSync(path, "utf8");
  markdown = markdown
    .replace("Design Status**: Draft", "Design Status**: Current")
    .replace(
      "- **Trello Sources**: https://example.test/card",
      "- **Trello Sources**: https://example.test/card\n- **Approver**: @linhai\n- **Approval Time**: 2026-08-13T00:00:00Z\n- **Approval Evidence**: Approve",
    );
  writeFileSync(path, markdown);
  const indexPath = join(root, "docs/architecture/INDEX.md");
  writeFileSync(
    indexPath,
    readFileSync(indexPath, "utf8").replace("| Draft |", "| Current |"),
  );
  return path;
}

test("rejects a Current ADD whose Architecture Design diagram is not a flowchart", () => {
  const root = validRepository();
  const path = currentAdd(root);
  writeFileSync(
    path,
    readFileSync(path, "utf8").replace(
      "flowchart LR\n  C1[\"C-1\"]",
      "sequenceDiagram\n  C1->>C1: C-1",
    ),
  );
  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /Architecture Design Mermaid diagram must be a flowchart/);
});

test("rejects a Current ADD Control Flow diagram of a disallowed type", () => {
  const root = validRepository();
  const path = currentAdd(root);
  const markdown = readFileSync(path, "utf8").replace(
    "## Control Flow Design [Conditionally Required — multi-step behavior]\nN/A — this fixture has no multi-step control flow.",
    [
      "## Control Flow Design [Conditionally Required — multi-step behavior]",
      "| ID | Flow |",
      "| --- | --- |",
      "| CF-1 | One flow |",
      "",
      "```mermaid",
      "stateDiagram-v2",
      "  CF1: CF-1",
      "```",
      "",
    ].join("\n"),
  );
  writeFileSync(path, markdown);
  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /Control Flow Design Mermaid diagram must be an allowed type for a Current ADD/);
});
