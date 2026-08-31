// ADR: docs/adr/ADR-0007-linear-time-governance-path-recognition.md

import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import { run, validRepository } from "./validate.test.mjs";

const validator = fileURLToPath(new URL("../validate.mjs", import.meta.url));

test("rejects a blank index Path without trying to read the repository root", () => {
  const root = validRepository();
  const path = join(root, "docs/adr/INDEX.md");
  const content = readFileSync(path, "utf8").replace(
    "N/A — governance-only example | docs/adr/ADR-0001-example.md |",
    "docs/architecture/ADD-0001-example.md — CAND-1 |  |",
  );
  writeFileSync(path, content);

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /index Path is missing/i);
  assert.doesNotMatch(result.stderr, /EISDIR/i);
});

test("rejects index Paths that are directories or escape the repository", () => {
  for (const [indexedPath, expected] of [
    ["docs/adr", /not a regular file/i],
    ["../../outside.md", /escapes the repository root/i],
  ]) {
    const root = validRepository();
    const path = join(root, "docs/adr/INDEX.md");
    writeFileSync(
      path,
      readFileSync(path, "utf8").replace("docs/adr/ADR-0001-example.md", indexedPath),
    );

    const result = run(root);
    assert.equal(result.status, 1);
    assert.match(result.stderr, expected);
  }
});

test("rejects an ADR Task Candidates pipe block without a Markdown separator", () => {
  const root = validRepository();
  const addPath = join(root, "docs/architecture/ADD-0001-example.md");
  writeFileSync(
    addPath,
    readFileSync(addPath, "utf8").replace(
      "| ID | Status | ADR path |\n| --- | --- | --- |",
      "| ID | Status | ADR path |",
    ),
  );

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /ADR Task Candidates.*structured Markdown table/i);
});

test("rejects an index pipe block without a Markdown separator", () => {
  const root = validRepository();
  const indexPath = join(root, "docs/architecture/INDEX.md");
  writeFileSync(
    indexPath,
    readFileSync(indexPath, "utf8").replace(
      "| --- | --- | --- | --- | --- | --- | --- | --- |\n",
      "",
    ),
  );

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /index requires a structured Markdown table/i);
});

test("rejects an adversarial index-path without timing out", () => {
  const root = validRepository();
  const indexPath = join(root, "docs/adr/INDEX.md");
  const adversarialPath = `${"segment/".repeat(24)}docs`;
  writeFileSync(
    indexPath,
    readFileSync(indexPath, "utf8").replace("docs/adr/ADR-0001-example.md", adversarialPath),
  );

  const result = spawnSync(process.execPath, [validator, "--root", root], {
    encoding: "utf8",
    timeout: 1_000,
  });

  assert.equal(result.error, undefined, result.error?.message);
  assert.equal(result.status, 1, result.stderr || result.stdout);
  assert.match(result.stderr, /index path/i);
});

test("rejects a candidate ADR link that resolves to a directory without crashing", () => {
  const root = validRepository();
  mkdirSync(join(root, "docs/adr/ADR-9999-directory.md"));
  const addPath = join(root, "docs/architecture/ADD-0001-example.md");
  writeFileSync(
    addPath,
    readFileSync(addPath, "utf8").replace(
      "| CAND-1 | Ready | None |",
      "| CAND-1 | Selected | docs/adr/ADR-9999-directory.md |",
    ),
  );

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /linked ADR path is not a regular file/i);
  assert.doesNotMatch(result.stderr, /EISDIR/i);
});

test("rejects an Architecture Source link that resolves to a directory without crashing", () => {
  const root = validRepository();
  mkdirSync(join(root, "docs/architecture/ADD-9999-directory.md"));
  const adrPath = join(root, "docs/adr/ADR-0001-example.md");
  writeFileSync(
    adrPath,
    readFileSync(adrPath, "utf8").replace(
      "Architecture Source**: N/A — governance-only example",
      "Architecture Source**: docs/architecture/ADD-9999-directory.md — CAND-1",
    ),
  );

  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /Architecture Source ADD path is not a regular file/i);
  assert.doesNotMatch(result.stderr, /EISDIR/i);
});
