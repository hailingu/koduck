// ADR: docs/adr/ADR-0002-required-ai-ci-postgres-verification.md

// Builds record-index and reciprocal-link validators from filesystem and
// Markdown parsing dependencies supplied by the CLI entry point.
export function createRelationshipValidator(context) {
  const {
    ADD_PATH_PATTERN,
    ADR_PATH_PATTERN,
    CANDIDATE_STATUSES,
    isCompleteValue,
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
    const idCol = header.findIndex((cell) => cell === "ID");
    const statusCol = header.findIndex((cell) => cell === "Status");
    const pathCol = header.findIndex((cell) => cell === "ADR path");
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
    const expectedCols = path.endsWith("docs/architecture/INDEX.md")
      ? ["ID", "Title", "Design Status", "Scope Level", "Scope", "Path", "Trello Source", "Superseded By"]
      : ["Type", "ID", "Title", "Decision Status", "Implementation Status", "Scope", "Architecture Source", "Path", "Superseded By"];
    for (const col of expectedCols) {
      if (!columns.has(col)) {
        errors.push(`${path}: index is missing required column ${col}`);
      }
    }
    for (const cells of parsed.rows) {
      const rowText = `| ${cells.join(" | ")} |`;
      const matches = [...rowText.matchAll(/`?((?:[^`|\s]+\/)*docs\/(?:adr|architecture)\/[^`|\s]+\.md)`?/g)];
      const indexed = columns.has("Path")
        ? cells[columns.get("Path")]
        : matches.at(-1)?.[1];
      if (!indexed) {
        errors.push(`${path}: index Path is missing`);
        continue;
      }
      if (seen.has(indexed)) errors.push(`${path}: duplicate index path ${indexed}`);
      seen.add(indexed);
      const absolute = resolveRepositoryFile(root, indexed, path, "index path", errors);
      if (!absolute) continue;
      const record = readFileSync(absolute, "utf8");
      // Status comparisons
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
      // Title comparison
      if (columns.has("Title")) {
        const indexedTitle = cells[columns.get("Title")];
        const h1Match = record.match(/^#\s+(?:Lightweight\s+)?(?:ADR|OCR|ADD)-\d+:\s*(.+)$/m);
        const recordTitle = h1Match ? h1Match[1].trim() : "";
        if (recordTitle && indexedTitle !== recordTitle) {
          errors.push(`${path}: index Title disagrees with record for ${indexed}: ${indexedTitle} vs ${recordTitle}`);
        }
      }
      // Type comparison (ADR/OCR index)
      if (columns.has("Type")) {
        const indexedType = cells[columns.get("Type")];
        const recordFilename = indexed.split("/").at(-1);
        let recordType;
        if (recordFilename.startsWith("OCR-")) recordType = "OCR";
        else if (/^#\s+Lightweight ADR-\d+/m.test(record)) recordType = "Lightweight ADR";
        else recordType = "Full ADR";
        if (indexedType !== recordType) {
          errors.push(`${path}: index Type ${indexedType} disagrees with record ${recordType} for ${indexed}`);
        }
      }
      // ID comparison — derive the record ID from its filename.
      if (columns.has("ID")) {
        const indexedId = cells[columns.get("ID")];
        const recordId = indexed.split("/").at(-1).match(/^(?:ADR|ADD|OCR)-\d+/)?.[0] ?? "";
        if (recordId && indexedId !== recordId) {
          errors.push(`${path}: index ID ${indexedId} disagrees with record ${recordId} for ${indexed}`);
        }
      }
      // Additional authoritative field comparisons — do not skip missing record
      // metadata; a missing field is itself a disagreement (AGENTS.md).
      const recordIsAdd = indexed.includes("/architecture/");
      const fieldMap = recordIsAdd
        ? [["Scope Level", "Scope Level"], ["Scope", "Scope"], ["Superseded By", "Superseded By"], ["Trello Source", "Trello Sources"]]
        : [["Scope", "Record Scope"], ["Architecture Source", "Architecture Source"], ["Superseded By", "Superseded By"]];
      for (const [column, field] of fieldMap) {
        if (!columns.has(column)) continue; // missing columns caught at header
        const indexedValue = (cells[columns.get(column)] ?? "").replace(/`/g, "");
        const recordValue = (metadata(record, field) ?? "").replace(/`/g, "");
        // For Trello Source, normalize Markdown links to their URL for comparison.
        if (column === "Trello Source") {
          const indexedUrl = indexedValue.match(/https?:\/\/[^\s)]+/)?.[0] ?? indexedValue;
          const recordUrl = recordValue.match(/https?:\/\/[^\s)]+/)?.[0] ?? recordValue;
          if (indexedUrl !== recordUrl) {
            errors.push(`${path}: index ${column} disagrees with record for ${indexed}`);
          }
        } else if (indexedValue !== recordValue) {
          errors.push(`${path}: index ${column} disagrees with record for ${indexed}`);
        }
      }
    }
    return seen;
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
    for (const [candidate, { status, path: linked }] of parsed.table) {
      if (!CANDIDATE_STATUSES.has(status)) {
        errors.push(`${path}: ${candidate} has illegal status ${status}`);
        continue;
      }
      if (status !== "Selected" && status !== "Complete") continue;
      const linkedPath = (linked.match(ADR_PATH_PATTERN) ?? [])[0];
      if (!linkedPath) {
        errors.push(`${path}: Selected or Complete ${candidate} is missing its linked ADR path`);
        continue;
      }
      const absolute = resolveRepositoryFile(root, linkedPath, path, "linked ADR path", errors);
      if (!absolute) continue;
      const adr = readFileSync(absolute, "utf8");
      const source = metadata(adr, "Architecture Source") ?? "";
      const sourceAdd = source.match(ADD_PATH_PATTERN)?.[0];
      const sourceCandidate = source.match(/\bCAND-\d+\b/)?.[0];
      if (sourceAdd !== path || sourceCandidate !== candidate) {
        errors.push(`${path}: reciprocal Architecture Source is missing for ${candidate} -> ${linkedPath}`);
      }
      // A Selected candidate's ADR must remain in an allowed non-terminal state;
      // a Complete candidate's ADR must be Complete or Verified.
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
      } else if (!["Complete", "Verified"].includes(implementation)) {
        errors.push(
          `${path}: ${candidate} is Complete but linked ADR ${linkedPath} is ${implementation ?? "<missing>"}`,
        );
      }
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
    const addPath = source.match(ADD_PATH_PATTERN)?.[0];
    const candidate = source.match(/\bCAND-\d+\b/)?.[0];
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
