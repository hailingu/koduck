// Builds terminal ADR and OCR evidence validators from shared lifecycle and
// Markdown table helpers.
export function createTerminalValidator(context) {
  const {
    RISK_DIMENSIONS,
    checklistItems,
    isCompleteValue,
    metadata,
    normalizeDimension,
    sectionContent,
    sectionTable,
    stripFencedCode,
  } = context;

  function validateCompletion(path, markdown, errors) {
    const isOcr = path.split("/").at(-1).startsWith("OCR-");
    const subtaskSection = isOcr ? "Task Definition" : "Implementation Plan";
    validateTerminalTable(path, markdown, subtaskSection, /^T-\d+$/, ["Complete"], "subtask", errors, 3);
    if (isOcr) {
      validateOcrRunbook(path, markdown, errors);
      validateOcrClosure(path, markdown, errors);
    } else {
      validateTerminalTable(path, markdown, "Acceptance Checks", /^AC-\d+$/, ["Pass"], "acceptance check", errors);
      validateCompletionChecklist(path, markdown, errors);
      validateTerminalRiskMatrix(path, markdown, errors);
    }
  }
  
  // Requires a section to contain a real table with ID, Status, and actual-
  // evidence columns, at least one valid ID row (and at most `maxRows`), every
  // row carrying an allowed terminal status (or N/A), and every Complete/Pass
  // row recording complete actual evidence. Illegal or duplicate IDs are rejected
  // rather than silently skipped (AGENTS.md).
  
  function validateTerminalTable(path, markdown, section, idPattern, allowed, label, errors, maxRows) {
    const table = sectionTable(markdown, section);
    if (!table) {
      errors.push(`${path}: ${section} requires a structured table for a terminal record`);
      return;
    }
    const idCol = table.header.findIndex((h) => /^Check ID$/i.test(h) || h === "ID");
    const statusCol = table.header.findIndex((h) => h === "Status");
    const evidenceCol = table.header.findLastIndex((h) => /^Actual.*evidence/i.test(h));
    if (idCol === -1 || statusCol === -1) {
      errors.push(`${path}: ${section} table is missing required ID or Status columns`);
      return;
    }
    if (evidenceCol === -1) {
      errors.push(`${path}: ${section} table is missing a required actual evidence column for a terminal record`);
      return;
    }
    const seen = new Set();
    let validCount = 0;
    for (const row of table.rows) {
      const id = row[idCol] ?? "";
      if (!idPattern.test(id)) {
        errors.push(`${path}: ${section} table has a row with a missing or illegal ID (${id || "<missing>"})`);
        continue;
      }
      if (seen.has(id)) {
        errors.push(`${path}: ${section} table duplicates ${label} ID ${id}`);
        continue;
      }
      seen.add(id);
      validCount += 1;
      const status = row[statusCol] ?? "";
      if (!isCompleteTerminal(status, allowed)) {
        errors.push(`${path}: ${label} ${id} must be ${allowed.join(" or ")} or N/A for a terminal record (is ${status})`);
        continue;
      }
      if (allowed.includes(status) && !isCompleteValue(row[evidenceCol])) {
        errors.push(`${path}: ${label} ${id} must record actual completion evidence (is ${row[evidenceCol] ?? "<missing>"})`);
      }
    }
    if (validCount === 0) {
      errors.push(`${path}: ${section} requires at least one ${label} row for a terminal record`);
    }
    if (maxRows !== undefined && validCount > maxRows) {
      errors.push(`${path}: ${section} must have at most ${maxRows} ${label} rows for a terminal record (has ${validCount})`);
    }
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
    const idCol = table.header.findIndex((h) => h === "ID");
    const statusCol = table.header.findIndex((h) => h === "Status");
    const evidenceCol = table.header.findLastIndex((h) => /^Actual.*evidence/i.test(h));
    if (idCol === -1 || statusCol === -1 || evidenceCol === -1) {
      errors.push(`${path}: Completion Checklist table is missing required ID, Status, or evidence columns`);
      return;
    }
    const seen = new Map();
    for (const row of table.rows) {
      const id = row[idCol] ?? "";
      if (!/^A-\d+$/.test(id)) continue;
      if (seen.has(id)) {
        errors.push(`${path}: Completion Checklist duplicates item ${id}`);
        continue;
      }
      seen.set(id, { status: row[statusCol] ?? "", row });
    }
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
      if (status === "Complete" && !isCompleteValue(row[evidenceCol])) {
        errors.push(`${path}: completion item ${id} must record actual completion evidence (is ${row[evidenceCol] ?? "<missing>"})`);
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
      const status = row[statusCol] ?? "";
      if (/^N\/A\s+—\s+\S/.test(status)) continue;
      if (!/^Pass\b/.test(status)) {
        errors.push(`${path}: Risk Coverage Matrix dimension ${dimension} must be Pass or N/A for a terminal record (is ${status})`);
        continue;
      }
      const evidence = evidenceCol !== -1
        ? (row[evidenceCol] ?? "")
        : status.replace(/^Pass\b[\s—-]*/, "").trim();
      if (!isCompleteValue(evidence)) {
        errors.push(`${path}: Risk Coverage Matrix dimension ${dimension} Pass row must record stable evidence (is ${evidence || "<missing>"})`);
      }
    }
  }
  
  // Returns the content under a `### <heading>` subsection within `content`,
  // up to the next heading of the same or higher level.
  function subsectionContent(content, heading) {
    const escaped = heading.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
    const match = new RegExp(`^### ${escaped}\\b[^\\n]*\\n`, "m").exec(content);
    if (!match) return undefined;
    const remainder = content.slice(match.index + match[0].length);
    const next = remainder.search(/^#{2,3} /m);
    return next === -1 ? remainder : remainder.slice(0, next);
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
      const result = stageContent.match(/\*\*Actual result and stable evidence\*\*:\s*([^\n]*)/)?.[1].trim();
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
  
  function closureField(content, field) {
    const escaped = field.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
    return new RegExp(`^- \\*\\*${escaped}\\*\\*:\\s*(.+)$`, "m").exec(content)?.[1].trim();
  }
  
  function isCompleteTerminal(status, allowed) {
    return allowed.includes(status) || /^N\/A\s+—\s+\S/.test(status);
  }
  
  // Parses the `ADR Task Candidates` table into candidate-id -> { status, path }
  // and reports any candidate ID that appears more than once. Status and ADR path
  // are read from their named table columns, never from evidence text that happens
  // to mention an ADR path elsewhere in the row. Returns undefined when the
  // section is absent or its header is missing/malformed.
  
  return { validateCompletion };
}
