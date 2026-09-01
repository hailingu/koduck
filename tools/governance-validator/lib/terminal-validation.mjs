// ADR: docs/adr/ADR-0002-required-ai-ci-postgres-verification.md

import { escapeRegExp } from "./escape-regexp.mjs";

// Returns whether a terminal status is the required terminal value or a
// reasoned N/A.
function isCompleteTerminal(status, allowed) {
  return allowed.includes(status) || /^N\/A\s+—\s+\S/.test(status);
}

// Reads one `### <heading>` subsection's Closure field value from Markdown
// list form (`- **Field**: value`).
function closureField(content, field) {
  const escaped = escapeRegExp(field);
  return new RegExp(String.raw`^- \*\*${escaped}\*\*:\s*(.+)$`, "m").exec(content)?.[1].trim();
}

// Locates a terminal table's ID, Status, and final actual-evidence columns.
function terminalTableColumns(table) {
  return {
    id: table.header.findIndex((h) => /^Check ID$/i.test(h) || h === "ID"),
    status: table.header.indexOf("Status"),
    evidence: table.header.findLastIndex((h) => /^Actual.*evidence/i.test(h)),
  };
}

// Collects the checklist's A-N rows by ID; duplicate IDs are reported.
function collectChecklistItems(path, table, columns, errors) {
  const seen = new Map();
  for (const row of table.rows) {
    const id = row[columns.id] ?? "";
    if (!/^A-\d+$/.test(id)) continue;
    if (seen.has(id)) {
      errors.push(`${path}: Completion Checklist duplicates item ${id}`);
      continue;
    }
    seen.set(id, { status: row[columns.status] ?? "", row });
  }
  return seen;
}


// Builds terminal ADR and OCR evidence validators from shared lifecycle and
// Markdown table helpers.
export function createTerminalValidator(context) {
  const {
    RISK_DIMENSIONS,
    isCompleteValue,
    normalizeDimension,
    sectionContent,
    sectionTable,
    stripFencedCode,
    subsectionContent,
  } = context;

  function validateCompletion(path, markdown, errors) {
    const isOcr = path.split("/").at(-1).startsWith("OCR-");
    const subtaskSection = isOcr ? "Task Definition" : "Implementation Plan";
    validateTerminalTable(path, markdown, {
      section: subtaskSection,
      idPattern: /^T-\d+$/,
      allowed: ["Complete"],
      label: "subtask",
      maxRows: 3,
    }, errors);
    if (isOcr) {
      validateOcrRunbook(path, markdown, errors);
      validateOcrClosure(path, markdown, errors);
    } else {
      validateTerminalTable(path, markdown, {
        section: "Acceptance Checks",
        idPattern: /^AC-\d+$/,
        allowed: ["Pass"],
        label: "acceptance check",
      }, errors);
      validateCompletionChecklist(path, markdown, errors);
      validateTerminalRiskMatrix(path, markdown, errors);
    }
  }

  // Requires a section to contain a real table with ID, Status, and actual-
  // evidence columns, at least one valid ID row (and at most `maxRows`), every
  // row carrying an allowed terminal status (or N/A), and every Complete/Pass
  // row recording complete actual evidence. Illegal or duplicate IDs are rejected
  // rather than silently skipped (AGENTS.md).
  function validateTerminalTable(path, markdown, contract, errors) {
    const table = sectionTable(markdown, contract.section);
    if (!table) {
      errors.push(`${path}: ${contract.section} requires a structured table for a terminal record`);
      return;
    }
    const columns = terminalTableColumns(table);
    if (columns.id === -1 || columns.status === -1) {
      errors.push(`${path}: ${contract.section} table is missing required ID or Status columns`);
      return;
    }
    if (columns.evidence === -1) {
      errors.push(`${path}: ${contract.section} table is missing a required actual evidence column for a terminal record`);
      return;
    }
    const seen = new Set();
    let validCount = 0;
    for (const row of table.rows) {
      validCount += terminalTableRowState(path, contract, columns, row, seen, errors);
    }
    if (validCount === 0) {
      errors.push(`${path}: ${contract.section} requires at least one ${contract.label} row for a terminal record`);
    }
    if (contract.maxRows !== undefined && validCount > contract.maxRows) {
      errors.push(`${path}: ${contract.section} must have at most ${contract.maxRows} ${contract.label} rows for a terminal record (has ${validCount})`);
    }
  }

  // Validates one terminal-table row and returns 1 when it declares a new
  // valid ID, 0 otherwise. Illegal or duplicate IDs are reported, not skipped.
  function terminalTableRowState(path, contract, columns, row, seen, errors) {
    const id = row[columns.id] ?? "";
    if (!contract.idPattern.test(id)) {
      errors.push(`${path}: ${contract.section} table has a row with a missing or illegal ID (${id || "<missing>"})`);
      return 0;
    }
    if (seen.has(id)) {
      errors.push(`${path}: ${contract.section} table duplicates ${contract.label} ID ${id}`);
      return 0;
    }
    seen.add(id);
    const status = row[columns.status] ?? "";
    if (!isCompleteTerminal(status, contract.allowed)) {
      errors.push(`${path}: ${contract.label} ${id} must be ${contract.allowed.join(" or ")} or N/A for a terminal record (is ${status})`);
      return 1;
    }
    if (contract.allowed.includes(status) && !isCompleteValue(row[columns.evidence])) {
      errors.push(`${path}: ${contract.label} ${id} must record actual completion evidence (is ${row[columns.evidence] ?? "<missing>"})`);
    }
    return 1;
  }

  // The template-defined Completion Checklist IDs required for a terminal ADR.
  const COMPLETION_CHECKLIST_IDS = ["A-1", "A-2", "A-3", "A-4", "A-5", "A-6", "A-7", "A-8"];
  // Items that must be Complete (not N/A) for a terminal record; only A-3 and A-6
  // are conditional ("when applicable") and may carry a reasoned N/A. For Full
  // ADRs, A-7 is also conditional; for Lightweight ADRs, A-7 is required.
  const FULL_MUST_COMPLETE = new Set(["A-1", "A-2", "A-4", "A-5", "A-8"]);
  const LIGHTWEIGHT_MUST_COMPLETE = new Set(["A-1", "A-2", "A-4", "A-5", "A-7", "A-8"]);

  // The Completion Checklist must contain exactly the template-defined A-1..A-8
  // items for a terminal record. A-1 (approval) must be Complete, not N/A. Every
  // Complete item must record complete actual evidence (AGENTS.md).
  function validateCompletionChecklist(path, markdown, errors) {
    const table = sectionTable(markdown, "Completion Checklist");
    if (!table) {
      errors.push(`${path}: Completion Checklist requires a structured table for a terminal record`);
      return;
    }
    const isLightweight = /^#\s+Lightweight ADR-\d+/m.test(markdown);
    const mustComplete = isLightweight ? LIGHTWEIGHT_MUST_COMPLETE : FULL_MUST_COMPLETE;
    const columns = {
      id: table.header.indexOf("ID"),
      status: table.header.indexOf("Status"),
      evidence: table.header.findLastIndex((h) => /^Actual.*evidence/i.test(h)),
    };
    if (columns.id === -1 || columns.status === -1 || columns.evidence === -1) {
      errors.push(`${path}: Completion Checklist table is missing required ID, Status, or evidence columns`);
      return;
    }
    const seen = collectChecklistItems(path, table, columns, errors);
    validateExpectedChecklistItems(path, seen, errors);
    validateChecklistItemStatuses(path, seen, mustComplete, columns, errors);
  }

  // The checklist must contain exactly the template-defined items — no
  // missing and no unexpected IDs.
  function validateExpectedChecklistItems(path, seen, errors) {
    const expected = new Set(COMPLETION_CHECKLIST_IDS);
    for (const id of COMPLETION_CHECKLIST_IDS) {
      if (!seen.has(id)) {
        errors.push(`${path}: Completion Checklist item ${id} is missing for a terminal record`);
      }
    }
    for (const id of seen.keys()) {
      if (!expected.has(id)) {
        errors.push(`${path}: Completion Checklist has unexpected item ${id}`);
      }
    }
  }

  // A-1 (approval) must be Complete; must-complete items may not be N/A; every
  // Complete item records complete actual evidence.
  function validateChecklistItemStatuses(path, seen, mustComplete, columns, errors) {
    const a1 = seen.get("A-1");
    if (a1 && a1.status !== "Complete") {
      errors.push(`${path}: Completion Checklist item A-1 must be Complete for a terminal record (is ${a1.status})`);
    }
    for (const [id, { status, row }] of seen) {
      if (!isCompleteTerminal(status, ["Complete"])) {
        errors.push(`${path}: completion item ${id} must be Complete or N/A for a terminal record (is ${status})`);
        continue;
      }
      if (mustComplete.has(id) && status !== "Complete") {
        errors.push(`${path}: completion item ${id} must be Complete for a terminal record (is ${status})`);
      }
      if (status === "Complete" && !isCompleteValue(row[columns.evidence])) {
        errors.push(`${path}: completion item ${id} must record actual completion evidence (is ${row[columns.evidence] ?? "<missing>"})`);
      }
    }
  }

  // A terminal ADR's Risk Coverage Matrix rows must each be Pass with stable
  // (non-placeholder) evidence or N/A with a reason; Not Started, Fail, or
  // Pending/placeholder evidence blocks completion (AGENTS.md). Handles both the
  // combined "Status and stable evidence" column and the separate Status +
  // "Actual evidence" columns.
  function validateTerminalRiskMatrix(path, markdown, errors) {
    const table = sectionTable(markdown, "Risk Coverage Matrix");
    if (!table) return; // a missing matrix is reported elsewhere
    const statusCol = table.header.findIndex((h) => h === "Status" || /^Status\b/.test(h));
    const evidenceCol = table.header.findIndex((h) => /^Actual evidence/i.test(h));
    if (statusCol === -1) {
      errors.push(`${path}: Risk Coverage Matrix is missing a Status column for a terminal record`);
      return;
    }
    const baseline = new Set(RISK_DIMENSIONS.map(normalizeDimension));
    for (const row of table.rows) {
      const dimension = row[0] ?? "";
      if (!baseline.has(normalizeDimension(dimension))) continue;
      validateTerminalRiskRow(path, row, dimension, statusCol, evidenceCol, errors);
    }
  }

  // One baseline dimension's terminal row must be Pass with stable evidence or
  // a reasoned N/A.
  function validateTerminalRiskRow(path, row, dimension, statusCol, evidenceCol, errors) {
    const status = row[statusCol] ?? "";
    if (/^N\/A\s+—\s+\S/.test(status)) return;
    if (!/^Pass\b/.test(status)) {
      errors.push(`${path}: Risk Coverage Matrix dimension ${dimension} must be Pass or N/A for a terminal record (is ${status})`);
      return;
    }
    const evidence = evidenceCol !== -1
      ? (row[evidenceCol] ?? "")
      : status.replace(/^Pass\b[\s—-]*/, "").trim();
    if (!isCompleteValue(evidence)) {
      errors.push(`${path}: Risk Coverage Matrix dimension ${dimension} Pass row must record stable evidence (is ${evidence || "<missing>"})`);
    }
  }

  // A terminal OCR's Core Runbook must record actual results for each of the four
  // required stages (Preflight, Execute, Verify, Stop and Recovery) — not merely
  // count same-named evidence fields (AGENTS.md).
  function validateOcrRunbook(path, markdown, errors) {
    const content = stripFencedCode(sectionContent(markdown, "Core Runbook And Evidence") ?? "");
    if (!content) {
      errors.push(`${path}: Core Runbook And Evidence is required for a terminal OCR`);
      return;
    }
    for (const stage of ["Preflight", "Execute", "Verify", "Stop and Recovery"]) {
      const stageContent = subsectionContent(content, stage);
      if (!stageContent) {
        errors.push(`${path}: Core Runbook And Evidence is missing required stage ${stage} for a terminal OCR`);
        continue;
      }
      const result = /\*\*Actual result and stable evidence\*\*:\s*([^\n]*)/.exec(stageContent)?.[1].trim();
      if (!isCompleteValue(result)) {
        errors.push(`${path}: Core Runbook And Evidence stage ${stage} must record actual result evidence for a terminal OCR`);
      }
    }
  }

  // A terminal OCR's Closure must record a complete Final result and Pass (not
  // Fail/Pending) for all four required review fields, including Governance
  // validation (AGENTS.md, OCR template).
  function validateOcrClosure(path, markdown, errors) {
    const content = stripFencedCode(sectionContent(markdown, "Closure") ?? "");
    if (!content) {
      errors.push(`${path}: Closure is required for a terminal OCR`);
      return;
    }
    const reviewFields = [
      "Authorization review",
      "Subtask and evidence review",
      "Requirement-level review",
      "Governance validation",
    ];
    for (const field of reviewFields) {
      const value = closureField(content, field);
      if (!isCompleteValue(value) || !/^Pass\b/.test(value)) {
        errors.push(`${path}: Closure field ${field} must be Pass for a terminal OCR (is ${value ?? "<missing>"})`);
      }
    }
    const finalResult = closureField(content, "Final result");
    if (!isCompleteValue(finalResult)) {
      errors.push(`${path}: Closure field Final result must record a complete terminal result for a terminal OCR`);
    }
  }

  return { validateCompletion };
}
