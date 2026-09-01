// ADR: docs/adr/ADR-0002-required-ai-ci-postgres-verification.md
// ADR: docs/adr/ADR-0014-validator-structural-parsing-reliability.md

// Structural Markdown parsing for the governance validator: fence-aware
// comment and code stripping, heading and record-filename recognition, and
// table, checklist, and section-content extraction. Every helper is pure and
// shared through the CLI entry point's dependency-injection contexts.

import { escapeRegExp } from "./escape-regexp.mjs";

// A CommonMark opening fence marker: up to three leading spaces followed by a
// run of at least three backticks or tildes.
export const FENCE_MARKER_PATTERN = /^\s{0,3}(`{3,}|~{3,})/;
// A line consisting solely of a closing fence run.
export const CLOSING_FENCE_PATTERN = /^\s{0,3}(`{3,}|~{3,})\s*$/;

// Determines whether a line's fence marker closes the given open fence: same
// marker character, at least as long, and nothing else on the line.
export function closesFence(marker, fence, line) {
  return marker !== null
    && marker[1].startsWith(fence.char)
    && marker[1].length >= fence.length
    && CLOSING_FENCE_PATTERN.test(line);
}

// Removes HTML-comment markers from one line, carrying the in-comment state
// across lines so a comment spanning lines stays inert.
export function stripHtmlCommentsFromLine(line, inComment) {
  let active = "";
  let cursor = 0;
  let htmlComment = inComment;
  while (cursor < line.length) {
    if (htmlComment) {
      const end = line.indexOf("-->", cursor);
      if (end === -1) return { line: active, htmlComment: true };
      htmlComment = false;
      cursor = end + 3;
      continue;
    }
    const start = line.indexOf("<!--", cursor);
    if (start === -1) {
      active += line.slice(cursor);
      break;
    }
    active += line.slice(cursor, start);
    htmlComment = true;
    cursor = start + 4;
  }
  return { line: active, htmlComment };
}

// Removes HTML comments outside real code fences before any structural parser
// sees Markdown. Fence parsing takes precedence, so comment markers in literal
// examples remain inert. Newlines are retained to preserve block boundaries.
export function stripHtmlComments(markdown) {
  const lines = markdown.split("\n");
  let fence = null;
  let htmlComment = false;
  const kept = [];
  for (const line of lines) {
    const marker = FENCE_MARKER_PATTERN.exec(line);
    if (fence !== null) {
      if (closesFence(marker, fence, line)) fence = null;
      kept.push(line);
      continue;
    }
    const stripped = stripHtmlCommentsFromLine(line, htmlComment);
    htmlComment = stripped.htmlComment;
    const activeMarker = FENCE_MARKER_PATTERN.exec(stripped.line);
    if (activeMarker) {
      fence = { char: activeMarker[1][0], length: activeMarker[1].length };
    }
    kept.push(stripped.line);
  }
  return kept.join("\n");
}

// Removes fenced examples after global HTML-comment sanitization so headings,
// metadata, tables, and checklists inside examples cannot become structure.
export function stripFencedCode(markdown) {
  const lines = stripHtmlComments(markdown).split("\n");
  let fence = null;
  const kept = [];
  for (const line of lines) {
    const marker = FENCE_MARKER_PATTERN.exec(line);
    if (fence !== null) {
      if (closesFence(marker, fence, line)) fence = null;
      continue;
    }
    if (marker) {
      fence = { char: marker[1][0], length: marker[1].length };
      continue;
    }
    kept.push(line);
  }
  return kept.join("\n");
}

// Finds a trailing bracketed suffix separated from its heading text.
export function trailingBracketSuffixStart(heading) {
  const trimmed = heading.trim();
  const opening = trimmed.lastIndexOf("[");
  return trimmed.endsWith("]") && opening > 0 && [" ", "\t"].includes(trimmed[opening - 1]) ? opening : -1;
}
// Collects only same-line level-two Markdown headings outside fenced code.
export function headingTexts(markdown) {
  const found = [];
  for (const line of stripFencedCode(markdown).split("\n")) {
    const heading = line.slice(2).trim();
    if (line.startsWith("##") && [" ", "\t"].includes(line[2]) && heading) found.push(heading);
  }
  return found;
}
export function headings(markdown) {
  return headingTexts(markdown).map(sectionName);
}
export function sectionName(heading) {
  const suffix = trailingBracketSuffixStart(heading);
  return (suffix === -1 ? heading : heading.slice(0, suffix)).trim();
}
// Determines whether a heading ends with a supported requirement-level label.
export function hasRequirementLevel(heading) {
  const suffix = trailingBracketSuffixStart(heading);
  if (suffix === -1) return false;
  const label = heading.trim().slice(suffix + 1, -1);
  return label === "Required" || label === "Optional" || (label.startsWith("Conditionally Required — ") && label !== "Conditionally Required — ");
}
// Determines whether a value begins with a decimal governance-record identifier.
export function hasRecordIdentifier(value, prefix) {
  const first = value[prefix.length];
  return value.startsWith(prefix) && first !== undefined && first >= "0" && first <= "9";
}
// Determines whether a filename belongs to a recognized governance record type.
export function isRecordFilename(filename, prefix) {
  return filename.endsWith(".md") && hasRecordIdentifier(filename, prefix);
}
// Determines whether the active document title selects the Lightweight ADR template.
export function isLightweightAdr(markdown) {
  const title = stripFencedCode(markdown).split("\n").find((line) => line.startsWith("# "));
  return title !== undefined && hasRecordIdentifier(title.slice(2).trim(), "Lightweight ADR-");
}
export function tableCells(line) {
  return line
    .slice(1, line.endsWith("|") ? -1 : undefined)
    .split("|")
    .map((cell) => cell.trim().replace(/^`|`$/g, ""));
}

export function isSeparatorRow(line) {
  return line.startsWith("|") && tableCells(line).every((cell) => /^:?-+:?$/.test(cell));
}

// Returns { header, rows } for the first markdown table in the section, with
// fenced code stripped so example tables inside code blocks cannot satisfy a
// structural check. Returns undefined when the section or its table is absent.
export function sectionTable(markdown, sectionName) {
  return tableFromContent(sectionContent(markdown, sectionName) ?? "");
}

export function subsectionTable(markdown, sectionName, subsectionName) {
  const section = sectionContent(markdown, sectionName) ?? "";
  return tableFromContent(subsectionContent(section, subsectionName) ?? "");
}

export function tableFromContent(markdown) {
  const content = stripFencedCode(markdown);
  if (!content) return undefined;
  const lines = content.split("\n");
  const headerIdx = lines.findIndex((line, index) => {
    const separator = lines[index + 1];
    return line.startsWith("|")
      && !isSeparatorRow(line)
      && separator !== undefined
      && isSeparatorRow(separator)
      && tableCells(separator).length === tableCells(line).length;
  });
  if (headerIdx === -1) return undefined;
  const tableLines = [];
  for (let index = headerIdx; index < lines.length && lines[index].startsWith("|"); index++) {
    tableLines.push(lines[index]);
  }
  return {
    header: tableCells(tableLines[0]),
    rows: tableLines.slice(1).filter((line) => !isSeparatorRow(line)).map(tableCells),
  };
}

// Parses a Markdown task-list item at the list margin or after three spaces.
export function checklistItem(line) {
  let index = 0;
  while (line[index] === " " && index < 3) index += 1;
  if (line[index] !== "-" || ![" ", "\t"].includes(line[index + 1]) || line[index + 2] !== "[") return undefined;
  const state = line[index + 3];
  if ((state !== " " && state !== "x" && state !== "X") || line[index + 4] !== "]") return undefined;
  const textStart = index + 5;
  if (line[textStart] !== undefined && ![" ", "\t"].includes(line[textStart])) return undefined;
  return { checked: state.toLowerCase() === "x", text: line.slice(textStart).trim() };
}

// Parses Markdown task-list items without letting a later item satisfy the
// requirement of an earlier checked item. Indented continuation lines remain
// part of the preceding item.
export function checklistItems(content) {
  const items = [];
  let current;
  for (const line of content.split("\n")) {
    const item = checklistItem(line);
    if (item) { current = item; items.push(current); } else if (current && line.trim()) {
      current.text += ` ${line.trim()}`;
    }
  }
  return items;
}

export function sectionContent(markdown, section) {
  const escaped = escapeRegExp(section);
  const match = new RegExp(String.raw`^## ${escaped}(?:\s+\[[^\]]+\])?\s*\n`, "m").exec(markdown);
  if (!match) return undefined;
  const remainder = markdown.slice(match.index + match[0].length);
  const next = remainder.search(/^## /m);
  return next === -1 ? remainder : remainder.slice(0, next);
}

// Returns the content under a named level-three subsection until the next
// level-two or level-three heading.
export function subsectionContent(content, heading) {
  const escaped = escapeRegExp(heading);
  const match = new RegExp(String.raw`^### ${escaped}\b[^\n]*\n`, "m").exec(content);
  if (!match) return undefined;
  const remainder = content.slice(match.index + match[0].length);
  const next = remainder.search(/^#{2,3} /m);
  return next === -1 ? remainder : remainder.slice(0, next);
}
