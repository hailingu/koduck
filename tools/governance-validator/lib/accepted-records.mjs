// ADR: docs/adr/archive/ADR-0009-accepted-record-validator-reliability.md

import { escapeRegExp } from "./escape-regexp.mjs";

// Classifies a record path and title so downstream validators share one source
// of truth for its template and lifecycle-specific requirements.
function recordKind(path, markdown) {
  const filename = path.split("/").at(-1);
  const isAdd = filename.startsWith("ADD-");
  const isOcr = filename.startsWith("OCR-");
  const isLightweight = !isAdd && !isOcr && /^#\s+Lightweight ADR-\d+/m.test(markdown);
  return { isAdd, isOcr, isLightweight };
}

// A standalone Pending placeholder anywhere in the body — not only as its
// first token — leaves a lifecycle-gated required section deliberately
// incomplete.
function containsStandalonePending(body) {
  // The placeholder is the capitalized token `Pending`, either as a whole
  // line or as the value of a Markdown field (`- **Owner**: Pending`); a
  // label prefix, bullet, quote, or emphasis never hides it. Lowercase
  // prose wrapping onto a line starting with "pending" is content.
  return body.split(/\n+/).some((line) => {
    const trimmed = line.trim();
    // Strip markdown scaffolding — bullets, quotes, emphasis, and a
    // field label ending in ":" — then require the placeholder at the
    // value position.
    const stripped = trimmed
      .replace(/^[-*_>]+\s*/, "")
      .replace(/^\*\*([^*]*)\*\*\s*:\s*/, "")
      .replace(/^[^:*]{0,80}:\s*/, "");
    if (/^\|.*\|$/.test(trimmed)) {
      const cells = trimmed
        .slice(1, -1)
        .split("|")
        .map((cell) => cell.trim());
      // Two-column field/value tables are narrative section content. Their
      // values cannot retain a lifecycle placeholder. Structured plan and
      // acceptance tables have their own status-aware validation below,
      // where Pending remains valid before a terminal implementation state.
      return cells.length === 2 && cells.some((cell) => /^Pending\b/.test(cell));
    }
    return /^Pending\b/.test(stripped);
  });
}

// Recognizes the repository's only valid table-level omission form: a
// reasoned N/A using the required em dash delimiter.
function isReasonedNa(value) {
  return /^N\/A\s+—\s+\S/.test(value);
}

// A runbook field's `**Field**:` value capture, from the field label through
// the next field label, stage heading, or section heading.
function runbookFieldPattern(escapedField) {
  return new RegExp(
    String.raw`\*\*${escapedField}\*\*:\s*([\s\S]*?)(?=\*\*[^*]+\*\*:\s*|^### |^## |$)`,
  );
}


// The template columns an Accepted subtask plan table must declare.
function subtaskRequiredHeaders(isOcr) {
  return isOcr
    ? [/^ID$/, /^Objective/i, /^Included scope/i, /^Completion criterion/i, /^Expected evidence/i, /^Status$/, /^Actual.*evidence/i]
    : [/^ID$/, /^Objective/i, /^Included scope/i, /^Status$/, /^Actual.*evidence/i];
}

// The template columns an Accepted acceptance-checks table must declare.
function acceptanceCheckRequiredHeaders() {
  return [
    /^Check ID$/i, /^Subtask$/i, /^Binary acceptance point$/i,
    /^Preconditions? or input$/i, /^Verification method$/i,
    /^Exact expected result$/i, /^Expected evidence$/i, /^Status$/i,
    /^Actual result and evidence$/i,
  ];
}

// The former three-column traceability shape preserved for completed
// historical records.
function isLegacyTraceabilityTable(table) {
  return table.header.some((header) => /^Normative contract clause$/i.test(header))
    && table.header.some((header) => /^Acceptance check or deterministic test$/i.test(header));
}

// Resolves the matrix's status, check-reference, applicability, and header
// columns, accepting the legacy terminal matrix shape; returns undefined
// when neither the current nor the legacy header contract is satisfied.
function riskMatrixColumns(table, isTerminalRecord) {
  const currentHeaders = [
    /^Risk dimension$/i,
    /^Applicability and scenario, or specific N\/A reason$/i,
    /^Owning boundary$/i,
    /^Deterministic verification method$/i,
    /^Exact expected result$/i,
    /^Acceptance check IDs$/i,
    /^Status$/i,
    /^Actual evidence$/i,
  ];
  const hasCurrentHeaders = currentHeaders.every((pattern) =>
    table.header.some((header) => pattern.test(header)),
  );
  const legacyHeaders = [
    /^Risk dimension$/i,
    /^Applicability and scenario$/i,
    /^Owning boundary$/i,
    /^Deterministic verification$/i,
    /^Exact expected result$/i,
    /^Checks$/i,
    /^Status and stable evidence$/i,
  ];
  const usesLegacyTerminalMatrix = isTerminalRecord
    && legacyHeaders.every((pattern) => table.header.some((header) => pattern.test(header)));
  if (!hasCurrentHeaders && !usesLegacyTerminalMatrix) return undefined;
  return {
    status: table.header.findIndex((header) => (
      usesLegacyTerminalMatrix ? /^Status and stable evidence$/i.test(header) : /^Status$/i.test(header)
    )),
    checkRefs: table.header.findIndex((header) => (
      usesLegacyTerminalMatrix ? /^Checks$/i.test(header) : /^Acceptance check IDs$/i.test(header)
    )),
    applicability: table.header.findIndex((header) => /^Applicability and scenario/i.test(header)),
    headers: table.header,
    usesLegacyTerminalMatrix,
  };
}


// Builds the accepted-stage ADR, OCR, and ADD content validator from shared
// Markdown parsing and lifecycle helpers.
export function createAcceptedRecordValidator(context) {
  const {
    ADD_CONDITIONAL_SECTIONS,
    ADD_REQUIRED_SECTIONS,
    CHECK_STATUSES,
    FULL_ADR_REQUIRED_SECTIONS,
    LIGHTWEIGHT_ADR_REQUIRED_SECTIONS,
    OCR_CONDITIONAL_SECTIONS,
    OCR_REQUIRED_SECTIONS,
    RISK_DIMENSIONS,
    SUBTASK_STATUSES,
    checklistItems,
    isCompleteTableValue,
    metadata,
    normalizeDimension,
    sectionContent,
    sectionTable,
    stripFencedCode,
    subsectionContent,
    subsectionTable,
    isCompleteValue,
    isSeparatorRow,
    tableCells,
  } = context;

  // These terminal records were accepted before stable touchpoints became a
  // required source/configuration gate. The explicit paths prevent lifecycle
  // status alone from becoming a bypass for newly drafted records.
  const LEGACY_TOUCHPOINT_EXEMPTIONS = new Set([
    "docs/adr/ADR-0001-provider-neutral-turn-kernel.md",
    "docs/adr/ADR-0002-required-ai-ci-postgres-verification.md",
  ]);
  const FULL_ADR_SOURCE_ONLY_SECTIONS = new Set([
    "Contract-To-Check Traceability",
    "Risk Coverage Matrix",
  ]);

  // The template's required top-level sections for one classified record kind.
  function requiredSectionsForKind(kind) {
    if (kind.isAdd) return ADD_REQUIRED_SECTIONS;
    if (kind.isOcr) return OCR_REQUIRED_SECTIONS;
    if (kind.isLightweight) return LIGHTWEIGHT_ADR_REQUIRED_SECTIONS;
    return FULL_ADR_REQUIRED_SECTIONS;
  }

  // Requires every top-level section mandated by the record template to carry
  // substantive accepted-stage content.
  function validateRequiredSectionBodies(path, markdown, kind, errors) {
    const required = requiredSectionsForKind(kind);
    for (const section of required) {
      const raw = sectionContent(markdown, section);
      if (raw === undefined) continue;
      const body = stripFencedCode(raw).trim();
      const allowsReasonedNa = !kind.isAdd && !kind.isOcr && !kind.isLightweight
        && FULL_ADR_SOURCE_ONLY_SECTIONS.has(section)
        && isReasonedNa(body);
      if (
        !body
        || containsStandalonePending(body)
        || (/^N\/A\b/i.test(body) && !allowsReasonedNa)
      ) {
        errors.push(`${path}: ${section} body must contain substantive content, not Pending or N/A`);
      }
    }
    validateRetainedOptionalSections(path, markdown, errors);
  }

  // Retained optional sections must carry complete content at final
  // lifecycle gates; removing the section entirely stays permitted.
  function validateRetainedOptionalSections(path, markdown, errors) {
    const seen = new Set();
    for (const match of markdown.matchAll(/^## ([^\n]+)$/gm)) {
      const heading = match[1].trim();
      if (!/\[Optional[^\]]*\]/.test(heading) || seen.has(heading)) continue;
      seen.add(heading);
      const raw = sectionContent(markdown, heading);
      if (raw === undefined) continue;
      const body = stripFencedCode(raw).trim();
      if (!body || containsStandalonePending(body) || (/^N\/A\b/.test(body) && !isReasonedNa(body))) {
        errors.push(
          `${path}: ${heading.split("[")[0].trim()} is a retained optional section and must contain complete content`,
        );
      }
    }
  }

  // Lifecycle-gated records retain every conditionally-required top-level
  // section so each trigger has either substantive content or a reasoned N/A.
  function validateConditionalSectionBodies(path, markdown, sections, errors) {
    for (const section of sections) {
      const raw = sectionContent(markdown, section);
      if (raw === undefined) {
        errors.push(`${path}: missing conditionally required section ${section}`);
        continue;
      }
      const body = stripFencedCode(raw).trim();
      if (
        !body
        || containsStandalonePending(body)
        || (/^N\/A\b/i.test(body) && !isReasonedNa(body))
      ) {
        errors.push(
          `${path}: ${section} body must contain substantive content or N/A — <reason>`,
        );
      }
    }
  }

  // Validates the one-to-three accepted subtasks and returns their declared IDs
  // for acceptance-check reference validation.
  function validateSubtaskPlan(path, markdown, isOcr, errors) {
    const section = isOcr ? "Task Definition" : "Implementation Plan";
    const table = sectionTable(markdown, section);
    const declared = new Set();
    if (!table) {
      errors.push(`${path}: ${section} requires a structured table for an Accepted record`);
      return declared;
    }
    const columns = {
      id: table.header.indexOf("ID"),
      status: table.header.indexOf("Status"),
      evidence: new Set(table.header.flatMap((header, index) =>
        /^Actual.*evidence/i.test(header) ? [index] : [],
      )),
    };
    const requiredHeaders = subtaskRequiredHeaders(isOcr);
    if (columns.id === -1 || columns.status === -1
      || !requiredHeaders.every((pattern) => table.header.some((header) => pattern.test(header)))) {
      errors.push(`${path}: ${section} table is missing required template columns for an Accepted record`);
      return declared;
    }
    for (const row of table.rows) {
      validateSubtaskRow(path, section, row, columns, declared, errors);
    }
    if (declared.size === 0) {
      errors.push(`${path}: ${section} requires at least one T-N subtask row for an Accepted record`);
    } else if (declared.size > 3) {
      errors.push(`${path}: ${section} must have at most 3 subtasks for an Accepted record (has ${declared.size})`);
    }
    return declared;
  }

  // Validates one subtask row and declares its ID; illegal or duplicate IDs
  // are rejected rather than silently skipped.
  function validateSubtaskRow(path, section, row, columns, declared, errors) {
    const id = row[columns.id] ?? "";
    if (!/^T-\d+$/.test(id)) {
      errors.push(`${path}: ${section} has a row with a missing or illegal ID (${id || "<missing>"})`);
      return;
    }
    if (declared.has(id)) {
      errors.push(`${path}: ${section} duplicates subtask ID ${id}`);
      return;
    }
    declared.add(id);
    const status = (row[columns.status] ?? "").trim();
    if (!SUBTASK_STATUSES.has(status) && !/^N\/A\s+—\s+\S/.test(status)) {
      errors.push(`${path}: subtask ${id} has an illegal Status (${status})`);
    }
    for (let index = 0; index < row.length; index++) {
      if (index === columns.id || index === columns.status || columns.evidence.has(index)) continue;
      if (!isCompleteTableValue((row[index] ?? "").trim())) {
        errors.push(`${path}: ${section} has an incomplete cell in a planned column for ${id}`);
        break;
      }
    }
  }

  // Requires source ADRs to identify stable implementation touchpoints while
  // retaining an explicit compatibility list for records accepted before this gate.
  function validateStableImplementationTouchpoints(path, markdown, isLightweight, errors) {
    const isTerminal = ["Complete", "Verified"].includes(metadata(markdown, "Implementation Status"));
    if (!isLightweight && isTerminal && LEGACY_TOUCHPOINT_EXEMPTIONS.has(path)) return;
    const subsection = subsectionContent(sectionContent(markdown, "Implementation Plan") ?? "", "Stable Implementation Touchpoints");
    if (!subsection) {
      errors.push(`${path}: Stable Implementation Touchpoints requires a structured table for an active Accepted ADR`);
      return;
    }
    const table = subsectionTable(markdown, "Implementation Plan", "Stable Implementation Touchpoints");
    const headers = [
      /^Path$/i,
      /^Stable symbol or contract anchor$/i,
      /^Key code excerpt, when needed$/i,
      /^Purpose$/i,
      /^Source revision$/i,
    ];
    if (!table) {
      errors.push(`${path}: Stable Implementation Touchpoints requires a structured table for an Accepted ADR`);
      return;
    }
    if (!headers.every((pattern) => table.header.some((header) => pattern.test(header)))) {
      errors.push(`${path}: Stable Implementation Touchpoints table is missing required template columns`);
      return;
    }
    if (table.rows.length === 0) {
      errors.push(`${path}: Stable Implementation Touchpoints requires at least one row for an Accepted ADR`);
      return;
    }
    const columns = headers.map((pattern) => table.header.findIndex((header) => pattern.test(header)));
    for (const row of table.rows) {
      const [pathValue, anchor, excerpt, purpose, revision] = columns.map((column) => (row[column] ?? "").trim());
      const hasAnchor = isCompleteTableValue(anchor);
      const hasExcerpt = isCompleteTableValue(excerpt);
      const requiredValuesComplete = [pathValue, purpose, revision]
        .every((value) => isCompleteTableValue(value));
      const anchorsValid = (hasAnchor || isReasonedNa(anchor))
        && (hasExcerpt || isReasonedNa(excerpt))
        && (hasAnchor || hasExcerpt);
      if (!requiredValuesComplete || !anchorsValid) {
        errors.push(`${path}: Stable Implementation Touchpoints has an incomplete required cell`);
        return;
      }
    }
  }

  // A Lightweight ADR always authorizes source behavior. A Full ADR is
  // governance-only only when every source/configuration-only section records
  // the required explicit non-applicability reason.
  function sourceConfigurationApplies(markdown, isLightweight) {
    if (isLightweight) return true;
    const implementationPlan = sectionContent(markdown, "Implementation Plan") ?? "";
    const sourceOnlyBodies = [
      subsectionContent(implementationPlan, "Stable Implementation Touchpoints"),
      sectionContent(markdown, "Contract-To-Check Traceability"),
      sectionContent(markdown, "Risk Coverage Matrix"),
    ];
    return !sourceOnlyBodies.every((body) => isReasonedNa(stripFencedCode(body ?? "").trim()));
  }

  // Validates accepted ADR checks and returns their IDs for traceability and
  // risk-matrix reference validation.
  function validateAcceptanceChecks(path, markdown, subtaskIds, errors) {
    const table = sectionTable(markdown, "Acceptance Checks");
    const declared = new Set();
    if (!table) {
      errors.push(`${path}: Acceptance Checks requires a structured table for an Accepted ADR`);
      return declared;
    }
    const columns = {
      id: table.header.findIndex((header) => /^Check ID$/i.test(header)),
      subtask: table.header.findIndex((header) => /^Subtask$/i.test(header)),
      status: table.header.findIndex((header) => /^Status$/i.test(header)),
      headers: table.header,
    };
    const requiredHeaders = acceptanceCheckRequiredHeaders();
    if (columns.id === -1 || columns.subtask === -1
      || !requiredHeaders.every((pattern) => table.header.some((header) => pattern.test(header)))) {
      errors.push(`${path}: Acceptance Checks table is missing required template columns`);
      return declared;
    }
    for (const row of table.rows) {
      validateAcceptanceCheckRow(path, row, columns, subtaskIds, declared, errors);
    }
    if (declared.size === 0) {
      errors.push(`${path}: Acceptance Checks requires at least one AC-N row for an Accepted ADR`);
    }
    return declared;
  }

  // Validates one acceptance-check row and declares its ID; illegal or
  // duplicate IDs are rejected rather than silently skipped.
  function validateAcceptanceCheckRow(path, row, columns, subtaskIds, declared, errors) {
    const id = row[columns.id] ?? "";
    if (!/^AC-\d+$/.test(id)) {
      errors.push(`${path}: Acceptance Checks has a row with a missing or illegal Check ID (${id || "<missing>"})`);
      return;
    }
    if (declared.has(id)) {
      errors.push(`${path}: Acceptance Checks duplicates ${id}`);
      return;
    }
    declared.add(id);
    if (!subtaskIds.has((row[columns.subtask] ?? "").trim())) {
      errors.push(`${path}: acceptance check ${id} Subtask must reference a declared T-N ID`);
    }
    const status = (row[columns.status] ?? "").trim();
    if (!CHECK_STATUSES.has(status) && !/^N\/A\s+—\s+\S/.test(status)) {
      errors.push(`${path}: acceptance check ${id} has an illegal Status (${status || "<missing>"})`);
    }
    for (let index = 0; index < columns.headers.length; index++) {
      const header = columns.headers[index];
      if (index === columns.id || index === columns.subtask || /^Status$/i.test(header)
        || /^Actual result and evidence$/i.test(header)) continue;
      if (!isCompleteTableValue((row[index] ?? "").trim())) {
        errors.push(`${path}: acceptance check ${id} has an incomplete planned column`);
        break;
      }
    }
  }

  // Enforces current traceability columns for active ADRs while preserving the
  // former three-column shape of completed historical records.
  function validateTraceability(path, markdown, checkIds, errors) {
    const table = sectionTable(markdown, "Contract-To-Check Traceability");
    const isTerminal = ["Complete", "Verified"].includes(metadata(markdown, "Implementation Status"));
    const isLegacy = isTerminal && table !== undefined && isLegacyTraceabilityTable(table);
    if (!table) {
      errors.push(`${path}: Contract-To-Check Traceability requires a structured table for an Accepted ADR`);
      return;
    }
    const columns = {
      clause: table.header.findIndex((header) => /^Clause ID$/i.test(header)),
      check: table.header.findIndex((header) => /Acceptance check/i.test(header)),
      columnCount: table.header.length,
    };
    const requiredHeaders = [
      /^Clause ID$/i, /^Authoritative contract path and heading$/i,
      /^Exact normative requirement$/i, /^Acceptance check or deterministic test IDs$/i,
      /^Explicit coverage method$/i,
    ];
    if (columns.clause === -1 || columns.check === -1
      || (!isLegacy && !requiredHeaders.every((pattern) => table.header.some((header) => pattern.test(header))))) {
      errors.push(`${path}: Contract-To-Check Traceability table is missing required template columns`);
      return;
    }
    const seen = new Set();
    for (const row of table.rows) {
      validateTraceabilityRow(path, row, columns, isLegacy, checkIds, seen, errors);
    }
    if (seen.size === 0) {
      errors.push(`${path}: Contract-To-Check Traceability requires at least one valid clause row for an Accepted ADR`);
    }
  }

  // Validates one traceability row and declares its clause ID; illegal or
  // duplicate IDs are rejected rather than silently skipped.
  function validateTraceabilityRow(path, row, columns, isLegacy, checkIds, seen, errors) {
    const clauseId = row[columns.clause] ?? "";
    if (!/^[A-Z]{2,}-\d+$/.test(clauseId)) {
      errors.push(`${path}: Contract-To-Check Traceability has a row with a missing or illegal Clause ID (${clauseId || "<missing>"})`);
      return;
    }
    if (seen.has(clauseId)) {
      errors.push(`${path}: Contract-To-Check Traceability duplicates ${clauseId}`);
      return;
    }
    seen.add(clauseId);
    const references = [...(row[columns.check] ?? "").matchAll(/\bAC-\d+\b/g)].map((match) => match[0]);
    if (references.length === 0) {
      errors.push(`${path}: clause ${clauseId} must reference at least one AC-N check`);
    } else {
      for (const checkId of references) {
        if (!checkIds.has(checkId)) errors.push(`${path}: clause ${clauseId} must reference declared acceptance check ${checkId}`);
      }
    }
    if (!isLegacy && hasIncompletePlannedColumn(row, columns.columnCount)) {
      errors.push(`${path}: clause ${clauseId} has an incomplete planned column`);
    }
  }

  // Returns whether any planned table cell is incomplete.
  function hasIncompletePlannedColumn(row, columnCount) {
    for (let index = 0; index < columnCount; index++) {
      if (!isCompleteTableValue((row[index] ?? "").trim())) return true;
    }
    return false;
  }

  // Requires the six template-defined OCR eligibility assertions to be checked
  // and to retain their normative boundary language.
  function validateOcrEligibility(path, markdown, errors) {
    const content = stripFencedCode(sectionContent(markdown, "Eligibility") ?? "");
    if (!content) return;
    const requirements = [
      {
        label: "boundary",
        matches: (text) => /\buses?\s+(?:(?:an?|the)\s+)?accepted\b/i.test(text)
          && ["architecture", "pipeline", "artifact", "contract", "security", "data"]
            .filter((term) => new RegExp(String.raw`\b${term}`, "i").test(text)).length >= 3,
      },
      { label: "reversible", matches: (text) => /\bis reversible\b/i.test(text) && /(recovery|rollback|discard|restore)/i.test(text) },
      {
        label: "Does not modify",
        matches: (text) => /Does not modify/i.test(text) && [
          "Dockerfile", "Makefile", "CI", "pipeline", "artifact format", "signing",
          "credential", "deployment", "API/schema/protocol", "authentication",
          "security policy", "data lifecycle", "dependenc", "provider", "irreversible",
        ].every((term) => text.toLowerCase().includes(term.toLowerCase())),
      },
      {
        label: "preflight",
        matches: (text) => /\bhas a defined preflight\b/i.test(text) && /\bsuccess check\b/i.test(text)
          && /\bstop condition\b/i.test(text) && /(recovery|rollback)/i.test(text),
      },
      {
        label: "no secret",
        matches: (text) => /\bcontains no secret\b/i.test(text) && /\bcredential\b/i.test(text)
          && /\bprivate endpoint\b/i.test(text) && /\bsensitive user data\b/i.test(text),
      },
      {
        label: "automatic",
        matches: (text) => (/\b(?:configured )?automatic(?:-review| review)(?: mechanism)? covers\b/i.test(text)
          || /N\/A\s+—\s+no automatic-review mechanism/i.test(text))
          && (/(exact (?:declared )?input (?:revision|commit))/i.test(text)
            || /N\/A\s+—\s+no automatic-review mechanism/i.test(text)),
      },
    ];
    const items = checklistItems(content);
    for (const [index, requirement] of requirements.entries()) {
      const item = items[index];
      if (!item?.checked || !requirement.matches(item.text)) {
        errors.push(`${path}: Eligibility item "${requirement.label}" must be present and confirmed for an Accepted OCR`);
      }
    }
  }

  // Requires every OCR runbook stage to retain all planned action, success, and
  // recovery fields before operational acceptance.
  function validateOcrRunbookPlan(path, markdown, errors) {
    const runbook = stripFencedCode(sectionContent(markdown, "Core Runbook And Evidence") ?? "");
    if (!runbook) return;
    const stageFields = {
      Preflight: ["Planned action and criterion"],
      Execute: ["Planned action"],
      Verify: ["Success criterion"],
      "Stop and Recovery": ["Stop condition", "Recovery action", "Recovery verification"],
    };
    for (const [stage, fields] of Object.entries(stageFields)) {
      const stageBody = subsectionContent(runbook, stage);
      if (!stageBody) {
        errors.push(`${path}: Core Runbook is missing required stage ${stage} for an Accepted OCR`);
        continue;
      }
      for (const field of fields) {
        const escaped = escapeRegExp(field);
        const match = runbookFieldPattern(escaped).exec(stageBody);
        if (!match || !isCompleteValue(match[1].trim())) {
          errors.push(`${path}: Core Runbook stage ${stage} field ${field} must be present and complete`);
        }
      }
      const actualResult = stageBody.match(/\*\*Actual result and stable evidence\*\*:\s*([^\n]+)/)?.[1].trim();
      if (!actualResult) {
        errors.push(`${path}: Core Runbook stage ${stage} field Actual result and stable evidence must be present`);
      }
    }
  }

  // Coordinates accepted-stage content validation without mixing the distinct
  // ADD, ADR, and OCR table contracts in one oversized function.
  function validateRequiredBodyContent(path, markdown, errors) {
    const kind = recordKind(path, markdown);
    validateRequiredSectionBodies(path, markdown, kind, errors);
    if (kind.isAdd) {
      validateConditionalSectionBodies(path, markdown, ADD_CONDITIONAL_SECTIONS, errors);
      return;
    }
    const subtaskIds = validateSubtaskPlan(path, markdown, kind.isOcr, errors);
    if (kind.isOcr) {
      validateConditionalSectionBodies(path, markdown, OCR_CONDITIONAL_SECTIONS, errors);
      validateOcrEligibility(path, markdown, errors);
      validateOcrRunbookPlan(path, markdown, errors);
      return;
    }
    const checkIds = validateAcceptanceChecks(path, markdown, subtaskIds, errors);
    if (!sourceConfigurationApplies(markdown, kind.isLightweight)) return;
    validateStableImplementationTouchpoints(path, markdown, kind.isLightweight, errors);
    validateTraceability(path, markdown, checkIds, errors);
    validateAcceptedRiskMatrix(path, markdown, checkIds, errors);
  }

  function validateRiskMatrixDimensions(path, markdown, errors) {
    const content = stripFencedCode(sectionContent(markdown, "Risk Coverage Matrix") ?? "");
    if (!content) return;
    const rows = content
      .split("\n")
      .filter((line) => line.startsWith("|") && !isSeparatorRow(line));
    if (rows.length === 0) {
      // A matrix with no table is only allowed as an explicit N/A with a reason;
      // otherwise the five-dimension coverage is mandatory.
      if (!isReasonedNa(content.trimStart())) {
        errors.push(`${path}: Risk Coverage Matrix must contain a five-dimension table or N/A — <reason>`);
      }
      return;
    }
    // The first non-separator row is the table header; the rest are dimension
    // rows. The matrix must contain each baseline dimension exactly once and no
    // extra dimension rows.
    const dataRows = rows.slice(1).map((line) => {
      const raw = tableCells(line)[0] ?? "";
      return { raw, normalized: normalizeDimension(raw) };
    });
    const counts = new Map();
    for (const { normalized } of dataRows) {
      counts.set(normalized, (counts.get(normalized) ?? 0) + 1);
    }
    const baseline = new Set(RISK_DIMENSIONS.map(normalizeDimension));
    for (const dimension of RISK_DIMENSIONS) {
      const count = counts.get(normalizeDimension(dimension)) ?? 0;
      if (count === 0) {
        errors.push(`${path}: Risk Coverage Matrix is missing baseline dimension ${dimension}`);
      } else if (count > 1) {
        errors.push(`${path}: Risk Coverage Matrix duplicates baseline dimension ${dimension}`);
      }
    }
    for (const { raw, normalized } of dataRows) {
      if (!baseline.has(normalized)) {
        errors.push(`${path}: Risk Coverage Matrix has a non-baseline dimension row: ${raw}`);
      }
    }
  }
  
  // Enforces the approval-stage Risk Coverage Matrix contract: every baseline
  // dimension must describe its scenario, owner, deterministic check, exact
  // result, and declared acceptance-check linkage before an ADR is Accepted.
  function validateAcceptedRiskMatrix(path, markdown, declaredCheckIds, errors) {
    const table = sectionTable(markdown, "Risk Coverage Matrix");
    if (!table) {
      errors.push(`${path}: Risk Coverage Matrix requires a structured table for an Accepted ADR`);
      return;
    }
    const isTerminalRecord = ["Complete", "Verified"].includes(
      metadata(markdown, "Implementation Status"),
    );
    const columns = riskMatrixColumns(table, isTerminalRecord);
    if (!columns) {
      errors.push(`${path}: Risk Coverage Matrix table is missing required template columns`);
      return;
    }
    const baseline = new Set(RISK_DIMENSIONS.map(normalizeDimension));
    for (const row of table.rows) {
      const dimension = row[0] ?? "";
      if (baseline.has(normalizeDimension(dimension))) {
        validateRiskMatrixRow(path, row, dimension, columns, declaredCheckIds, errors);
      }
    }
  }

  // Validates one baseline-dimension row's status, planned columns, and
  // declared acceptance-check references.
  function validateRiskMatrixRow(path, row, dimension, columns, declaredCheckIds, errors) {
    const status = (row[columns.status] ?? "").trim();
    const normalizedStatus = columns.usesLegacyTerminalMatrix
      ? (status.match(/^(Pass|Fail|Not Started|In Progress|Blocked)(?:\s+[—-].*)?$/)?.[1]
        ?? status)
      : status;
    if (!CHECK_STATUSES.has(normalizedStatus) && !/^N\/A\s+—\s+\S/.test(normalizedStatus)) {
      errors.push(`${path}: Risk Coverage Matrix dimension ${dimension} has an illegal Status (${status || "<missing>"})`);
    }
    validateRiskMatrixPlannedColumns(path, row, dimension, columns, errors);
    const references = [...(row[columns.checkRefs] ?? "").matchAll(/\bAC-\d+\b/g)].map((match) => match[0]);
    if (references.length === 0) {
      errors.push(`${path}: Risk Coverage Matrix dimension ${dimension} must reference at least one AC-N check`);
      return;
    }
    for (const checkId of references) {
      if (!declaredCheckIds.has(checkId)) {
        errors.push(`${path}: Risk Coverage Matrix dimension ${dimension} must reference declared acceptance check ${checkId}`);
      }
    }
  }

  // Every planned column must be complete; the applicability column may carry
  // a reasoned N/A, and status/evidence columns carry stage-dependent values.
  function validateRiskMatrixPlannedColumns(path, row, dimension, columns, errors) {
    for (let i = 0; i < columns.headers.length; i++) {
      const header = columns.headers[i];
      if (
        /^Status$/i.test(header)
        || /^Actual evidence$/i.test(header)
        || (columns.usesLegacyTerminalMatrix && /^Status and stable evidence$/i.test(header))
      ) continue;
      const value = (row[i] ?? "").trim();
      if (i === columns.applicability && isReasonedNa(value)) continue;
      if (!isCompleteTableValue(value)) {
        errors.push(`${path}: Risk Coverage Matrix dimension ${dimension} has an incomplete planned column`);
        break;
      }
    }
  }
  return { validateRequiredBodyContent, validateRiskMatrixDimensions };
}
