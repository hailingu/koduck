import assert from "node:assert/strict";
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import test from "node:test";

import {
  acceptedAdr,
  replaceSection,
  run,
  validRepository,
} from "./validate.test.mjs";

// Replaces all three source/configuration-only sections with the explicit
// non-applicability form required for a governance-only Full ADR.
function makeGovernanceOnly(root, id) {
  const path = join(root, `docs/adr/ADR-${id}-example.md`);
  let content = readFileSync(path, "utf8").replace(
    /### Stable Implementation Touchpoints[^\n]*\n\| Path[\s\S]*?\| working tree before commit \|\n/,
    "### Stable Implementation Touchpoints [Conditionally Required — source or configuration implementation]\nN/A — governance documentation only.\n",
  );
  content = replaceSection(
    content,
    "Contract-To-Check Traceability",
    "N/A — this governance-only ADR defines no source or configuration contract.",
  );
  content = replaceSection(
    content,
    "Risk Coverage Matrix",
    "N/A — this governance-only ADR has no source or configuration implementation risks.",
  );
  writeFileSync(path, content);
}

// Completes the fixture without changing its source/configuration
// applicability, allowing terminal-only validation behavior to be isolated.
function makeTerminalSourceAdr(root, id) {
  const path = join(root, `docs/adr/ADR-${id}-example.md`);
  let content = readFileSync(path, "utf8")
    .replace("Implementation Status**: In Progress", "Implementation Status**: Complete")
    .replace(
      "| T-1 | Implement the rule | validator | Not Started | Pending |",
      "| T-1 | Implement the rule | validator | Complete | implementation evidence |",
    )
    .replace(
      "| AC-1 | T-1 | The validator rejects the invalid record. | Invalid fixture. | Run the validator. | Exit status is 1. | Validator output. | Not Started | Pending |",
      "| AC-1 | T-1 | The validator rejects the invalid record. | Invalid fixture. | Run the validator. | Exit status is 1. | Validator output. | Pass | command evidence |",
    )
    .replaceAll("| Not Started | Not run — implementation not started. |", "| Pass | command evidence |")
    .replace(
      "## Completion Checklist [Required]\nChecklist.",
      `## Completion Checklist [Required]
| ID | Item | Status | Actual Evidence |
| --- | --- | --- | --- |
| A-1 | approved | Complete | approval evidence |
| A-2 | delivered | Complete | implementation evidence |
| A-3 | architecture source | Complete | traceability evidence |
| A-4 | requirement levels | Complete | review evidence |
| A-5 | acceptance checks | Complete | deterministic checks |
| A-6 | engineering exceptions | Complete | no exception evidence |
| A-7 | contracts and risks | Complete | matrix evidence |
| A-8 | governance validation | Complete | command evidence |`,
    )
    .replace(
      /### Stable Implementation Touchpoints[^\n]*\n\| Path[\s\S]*?\| working tree before commit \|\n/,
      "",
    );
  writeFileSync(path, content);
  const index = join(root, "docs/adr/INDEX.md");
  writeFileSync(
    index,
    readFileSync(index, "utf8").replace(
      `| Full ADR | ADR-${id} | Example | Accepted | In Progress |`,
      `| Full ADR | ADR-${id} | Example | Accepted | Complete |`,
    ),
  );
}

test("accepts reasoned N/A source-only sections in a governance Full ADR", () => {
  const root = validRepository();
  acceptedAdr(root, "0023");
  makeGovernanceOnly(root, "0023");

  const result = run(root);
  assert.equal(result.status, 0, result.stderr || result.stdout);
});

test("accepts a reasoned N/A applicability for one risk dimension", () => {
  const root = validRepository();
  acceptedAdr(root, "0024");
  const path = join(root, "docs/adr/ADR-0024-example.md");
  writeFileSync(
    path,
    readFileSync(path, "utf8").replace(
      "| concurrency and ordering | Applicable — concurrent calls. | Validator | Run the validator. | One valid result. | AC-1 | Not Started | Not run — implementation not started. |",
      "| concurrency and ordering | N/A — no concurrent behavior exists. | Validator | Inspect the single-threaded boundary. | No concurrent transition exists. | AC-1 | N/A — no concurrent behavior exists. | Not applicable by inspection. |",
    ),
  );

  const result = run(root);
  assert.equal(result.status, 0, result.stderr || result.stdout);
});

test("requires touchpoints from a newly terminal source Full ADR", () => {
  const root = validRepository();
  acceptedAdr(root, "0025");
  makeTerminalSourceAdr(root, "0025");

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /Stable Implementation Touchpoints requires a structured table/i);
});

test("rejects a touchpoint pipe block without a Markdown separator row", () => {
  const root = validRepository();
  acceptedAdr(root, "0026");
  const path = join(root, "docs/adr/ADR-0026-example.md");
  writeFileSync(
    path,
    readFileSync(path, "utf8").replace(
      "| Path | Stable symbol or contract anchor | Key code excerpt, when needed | Purpose | Source revision |\n| --- | --- | --- | --- | --- |",
      "| Path | Stable symbol or contract anchor | Key code excerpt, when needed | Purpose | Source revision |",
    ),
  );

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /Stable Implementation Touchpoints requires a structured table/i);
});

test("accepts a valid touchpoint table after pipe-delimited prose", () => {
  const root = validRepository();
  acceptedAdr(root, "0027");
  const path = join(root, "docs/adr/ADR-0027-example.md");
  writeFileSync(
    path,
    readFileSync(path, "utf8").replace(
      "| Path | Stable symbol or contract anchor |",
      "| supporting prose, not a table |\n| Path | Stable symbol or contract anchor |",
    ),
  );

  const result = run(root);
  assert.equal(result.status, 0, result.stderr || result.stdout);
});
