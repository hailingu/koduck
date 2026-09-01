// ADR: docs/adr/ADR-0013-relationship-validation-reliability.md

// Separates Markdown-delimited path candidates without matching through their
// contents, so record recognition remains bounded by the existing delimiters.
function recordPathTokens(value) {
  return String(value).split(/[|`\s()[\]{}<>]+/).filter(Boolean);
}

// Returns the non-empty title from a recognized record H1 without matching a
// greedy pattern across the full Markdown document.
function recordTitleFromHeading(markdown) {
  for (const line of String(markdown).split("\n")) {
    const title = recordTitleFromLine(line);
    if (title !== undefined) return title;
  }
  return "";
}

// Parses one heading line of the form `# [Lightweight ]ADR-NNNN: Title` and
// returns its title, or undefined when the line is not a recognized record H1.
function recordTitleFromLine(line) {
  if (!line.startsWith("#")) return undefined;
  let index = 1;
  while (line[index] === " " || line[index] === "\t") index += 1;
  if (index === 1) return undefined;

  if (line.startsWith("Lightweight ", index)) index += "Lightweight ".length;
  const recordPrefix = ["ADR-", "OCR-", "ADD-"].find((prefix) => line.startsWith(prefix, index));
  if (!recordPrefix) return undefined;
  index += recordPrefix.length;

  const idStart = index;
  while (line[index] >= "0" && line[index] <= "9") index += 1;
  if (index === idStart || line[index] !== ":") return undefined;

  const title = line.slice(index + 1).trim();
  return title || undefined;
}

// Returns the complete token for one ADR or ADD path when it has the expected
// record directory and filename prefix; callers retain ownership of resolution.
export function findRecordPath(value, directory, filenamePrefix) {
  for (const token of recordPathTokens(value)) {
    const filename = token.slice(token.lastIndexOf("/") + 1);
    if (
      token.includes(directory)
      && filename.startsWith(filenamePrefix)
      && /\d/.test(filename[filenamePrefix.length] ?? "")
      && filename.endsWith(".md")
    ) {
      return token;
    }
  }
  return undefined;
}

// The index columns each INDEX.md template must declare.
function expectedIndexColumns(path) {
  return path.endsWith("docs/architecture/INDEX.md")
    ? ["ID", "Title", "Design Status", "Scope Level", "Scope", "Path", "Trello Source", "Superseded By"]
    : ["Type", "ID", "Title", "Decision Status", "Implementation Status", "Scope", "Architecture Source", "Path", "Superseded By"];
}

// Resolves the row's record path from the Path column when present, falling
// back to the last recognized record token in the row.
function indexedRecordPath(cells, columns, rowText) {
  if (columns.has("Path")) return cells[columns.get("Path")];
  return recordPathTokens(rowText)
    .findLast((token) =>
      findRecordPath(token, "docs/adr/", "ADR-")
      || findRecordPath(token, "docs/architecture/", "ADD-"),
    );
}

// Title comparison
function validateIndexedTitle(path, cells, columns, indexed, record, errors) {
  if (!columns.has("Title")) return;
  const indexedTitle = cells[columns.get("Title")];
  const recordTitle = recordTitleFromHeading(record);
  if (recordTitle && indexedTitle !== recordTitle) {
    errors.push(`${path}: index Title disagrees with record for ${indexed}: ${indexedTitle} vs ${recordTitle}`);
  }
}

// Derives an index row's record type from its filename and title heading.
function indexRecordType(indexed, record) {
  if (indexed.split("/").at(-1).startsWith("OCR-")) return "OCR";
  if (/^#\s+Lightweight ADR-\d+/m.test(record)) return "Lightweight ADR";
  return "Full ADR";
}

// Type comparison (ADR/OCR index)
function validateIndexedType(path, cells, columns, indexed, record, errors) {
  if (!columns.has("Type")) return;
  const indexedType = cells[columns.get("Type")];
  const recordType = indexRecordType(indexed, record);
  if (indexedType !== recordType) {
    errors.push(`${path}: index Type ${indexedType} disagrees with record ${recordType} for ${indexed}`);
  }
}

// ID comparison — derive the record ID from its filename.
function validateIndexedId(path, cells, columns, indexed, errors) {
  if (!columns.has("ID")) return;
  const indexedId = cells[columns.get("ID")];
  const recordId = /^(?:ADR|ADD|OCR)-\d+/.exec(indexed.split("/").at(-1))?.[0] ?? "";
  if (recordId && indexedId !== recordId) {
    errors.push(`${path}: index ID ${indexedId} disagrees with record ${recordId} for ${indexed}`);
  }
}

// For Trello Source, normalize Markdown links to their URL for comparison.
function compareIndexedTrelloSource(path, column, indexed, indexedValue, recordValue, errors) {
  const indexedUrl = /https?:\/\/[^\s)]+/.exec(indexedValue)?.[0] ?? indexedValue;
  const recordUrl = /https?:\/\/[^\s)]+/.exec(recordValue)?.[0] ?? recordValue;
  if (indexedUrl !== recordUrl) {
    errors.push(`${path}: index ${column} disagrees with record for ${indexed}`);
  }
}

// Builds record-index and reciprocal-link validators from filesystem and
// Markdown parsing dependencies supplied by the CLI entry point.
export function createRelationshipValidator(context) {
  const {
    CANDIDATE_STATUSES,
    metadata,
    readFileSync,
    resolveRepositoryFile,
    sectionContent,
    tableFromContent,
  } = context;

  function candidateTable(markdown) {
    const parsed = tableFromContent(sectionContent(markdown, "ADR Task Candidates") ?? "");
    if (!parsed) return undefined;
    const { header, rows } = parsed;
    const idCol = header.indexOf("ID");
    const statusCol = header.indexOf("Status");
    const pathCol = header.indexOf("ADR path");
    if (idCol === -1 || statusCol === -1 || pathCol === -1) return undefined;
    const table = new Map();
    const seen = new Set();
    const duplicateIds = [];
    const malformedIds = [];
    for (const cells of rows) {
      const id = cells[idCol];
      if (!id || !/^CAND-\d+$/.test(id)) {
        malformedIds.push(id || "<missing>");
        continue;
      }
      if (seen.has(id)) {
        duplicateIds.push(id);
      } else {
        seen.add(id);
      }
      table.set(id, { status: cells[statusCol] ?? "", path: cells[pathCol] ?? "" });
    }
    return { table, duplicateIds, malformedIds };
  }

  function validateIndex(root, path, markdown, errors) {
    const seen = new Set();
    const parsed = tableFromContent(markdown);
    if (!parsed) {
      errors.push(`${path}: index requires a structured Markdown table`);
      return seen;
    }
    const columns = new Map(parsed.header.map((cell, index) => [cell, index]));
    for (const col of expectedIndexColumns(path)) {
      if (!columns.has(col)) {
        errors.push(`${path}: index is missing required column ${col}`);
      }
    }
    for (const cells of parsed.rows) {
      validateIndexRow(root, path, cells, columns, seen, errors);
    }
    return seen;
  }

  // Validates one index row: resolves its indexed record and compares every
  // authoritative field against the record's own content.
  function validateIndexRow(root, path, cells, columns, seen, errors) {
    const indexed = indexedRecordPath(cells, columns, `| ${cells.join(" | ")} |`);
    if (!indexed) {
      errors.push(`${path}: index Path is missing`);
      return;
    }
    if (seen.has(indexed)) errors.push(`${path}: duplicate index path ${indexed}`);
    seen.add(indexed);
    const absolute = resolveRepositoryFile(root, indexed, path, "index path", errors);
    if (!absolute) return;
    const record = readFileSync(absolute, "utf8");
    validateIndexedStatus(path, cells, columns, indexed, record, errors);
    validateIndexedTitle(path, cells, columns, indexed, record, errors);
    validateIndexedType(path, cells, columns, indexed, record, errors);
    validateIndexedId(path, cells, columns, indexed, errors);
    validateIndexedFields(path, cells, columns, indexed, record, errors);
  }

  // Status columns must agree with the indexed record's active metadata.
  function validateIndexedStatus(path, cells, columns, indexed, record, errors) {
    for (const [column, field] of [
      ["Decision Status", "Decision Status"],
      ["Implementation Status", "Implementation Status"],
      ["Design Status", "Design Status"],
    ]) {
      if (!columns.has(column)) continue;
      const indexedStatus = cells[columns.get(column)];
      const recordStatus = metadata(record, field);
      if (recordStatus && indexedStatus !== recordStatus) {
        errors.push(`${path}: index ${column} ${indexedStatus} disagrees with record ${recordStatus} for ${indexed}`);
      }
    }
  }

  // Additional authoritative field comparisons — do not skip missing record
  // metadata; a missing field is itself a disagreement (AGENTS.md).
  function validateIndexedFields(path, cells, columns, indexed, record, errors) {
    const recordIsAdd = indexed.includes("/architecture/");
    const fieldMap = recordIsAdd
      ? [["Scope Level", "Scope Level"], ["Scope", "Scope"], ["Superseded By", "Superseded By"], ["Trello Source", "Trello Sources"]]
      : [["Scope", "Record Scope"], ["Architecture Source", "Architecture Source"], ["Superseded By", "Superseded By"]];
    for (const [column, field] of fieldMap) {
      if (!columns.has(column)) continue; // missing columns caught at header
      const indexedValue = (cells[columns.get(column)] ?? "").replaceAll("`", "");
      const recordValue = (metadata(record, field) ?? "").replaceAll("`", "");
      if (column === "Trello Source") {
        compareIndexedTrelloSource(path, column, indexed, indexedValue, recordValue, errors);
      } else if (indexedValue !== recordValue) {
        errors.push(`${path}: index ${column} disagrees with record for ${indexed}`);
      }
    }
  }

  function validateReciprocalLinks(root, path, markdown, errors) {
    if (!path.includes("/architecture/") || !path.split("/").at(-1).startsWith("ADD-")) return;
    // A missing section is already flagged by required-sections validation; a
    // section that exists but cannot be parsed (missing/misspelled ID, Status, or
    // ADR path header) must not silently bypass the reciprocal-link check.
    if (!sectionContent(markdown, "ADR Task Candidates")) return;
    const parsed = candidateTable(markdown);
    if (!parsed) {
      errors.push(
        `${path}: ADR Task Candidates table is missing required ID, Status, or ADR path columns, or is not a structured Markdown table`,
      );
      return;
    }
    for (const id of parsed.malformedIds) {
      errors.push(`${path}: ADR Task Candidates table has a row with a missing or illegal ID (${id})`);
    }
    for (const id of parsed.duplicateIds) {
      errors.push(`${path}: ADR Task Candidates table has a duplicate candidate ID ${id}`);
    }
    for (const [candidate, entry] of parsed.table) {
      validateCandidateLink(root, path, candidate, entry, errors);
    }
  }

  // Validates one candidate row's linked ADR when its status requires a link.
  function validateCandidateLink(root, path, candidate, { status, path: linked }, errors) {
    if (!CANDIDATE_STATUSES.has(status)) {
      errors.push(`${path}: ${candidate} has illegal status ${status}`);
      return;
    }
    if (status !== "Selected" && status !== "Complete") return;
    const linkedPath = findRecordPath(linked, "docs/adr/", "ADR-");
    if (!linkedPath) {
      errors.push(`${path}: Selected or Complete ${candidate} is missing its linked ADR path`);
      return;
    }
    const absolute = resolveRepositoryFile(root, linkedPath, path, "linked ADR path", errors);
    if (!absolute) return;
    const adr = readFileSync(absolute, "utf8");
    validateReciprocalSource(path, candidate, linkedPath, adr, errors);
    validateLinkedAdrStatus(path, candidate, status, linkedPath, adr, errors);
  }

  // The linked ADR's Architecture Source must reciprocate this exact ADD path
  // and candidate.
  function validateReciprocalSource(path, candidate, linkedPath, adr, errors) {
    const source = metadata(adr, "Architecture Source") ?? "";
    const sourceAdd = findRecordPath(source, "docs/architecture/", "ADD-");
    const sourceCandidate = /\bCAND-\d+\b/.exec(source)?.[0];
    if (sourceAdd !== path || sourceCandidate !== candidate) {
      errors.push(`${path}: reciprocal Architecture Source is missing for ${candidate} -> ${linkedPath}`);
    }
  }

  // A Selected candidate's ADR must remain in an allowed non-terminal state;
  // a Complete candidate's ADR must be Complete or Verified.
  function validateLinkedAdrStatus(path, candidate, status, linkedPath, adr, errors) {
    const decision = metadata(adr, "Decision Status");
    const implementation = metadata(adr, "Implementation Status");
    if (status === "Selected") {
      if (
        !["Proposed", "Accepted"].includes(decision)
        || !["Not Started", "In Progress", "Blocked"].includes(implementation)
      ) {
        errors.push(
          `${path}: ${candidate} is Selected but linked ADR ${linkedPath} is ${decision ?? "<missing>"}/${implementation ?? "<missing>"}`,
        );
      }
      return;
    }
    if (!["Complete", "Verified"].includes(implementation)) {
      errors.push(
        `${path}: ${candidate} is Complete but linked ADR ${linkedPath} is ${implementation ?? "<missing>"}`,
      );
    }
  }

  function validateArchitectureSource(root, path, markdown, errors) {
    const source = metadata(markdown, "Architecture Source");
    if (source === undefined) {
      errors.push(`${path}: Architecture Source is missing`);
      return;
    }
    // Governance, process, and other non-product-demand ADRs may record
    // `N/A — <reason>`. Any other value must be an exact ADD path plus candidate.
    if (/^N\/A\s+—\s+\S/.test(source)) return;
    const addPath = findRecordPath(source, "docs/architecture/", "ADD-");
    const candidate = /\bCAND-\d+\b/.exec(source)?.[0];
    if (!addPath || !candidate) {
      errors.push(
        `${path}: Architecture Source must be an ADD path plus candidate ID or N/A — <reason>`,
      );
      return;
    }
    const absolute = resolveRepositoryFile(
      root,
      addPath,
      path,
      "Architecture Source ADD path",
      errors,
    );
    if (!absolute) return;
    const entry = candidateTable(readFileSync(absolute, "utf8"))?.table?.get(candidate);
    if (
      !entry
      || (entry.status !== "Selected" && entry.status !== "Complete")
      || entry.path !== path
    ) {
      errors.push(`${path}: reciprocal ADD candidate link is missing for ${addPath} — ${candidate}`);
    }
  }

  return { candidateTable, validateIndex, validateReciprocalLinks, validateArchitectureSource };
}
