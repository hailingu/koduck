// ADR: docs/adr/ADR-0010-mermaid-fence-recognition-reliability.md

// A CommonMark fence line consisting solely of a closing marker run.
const CLOSING_FENCE_PATTERN = /^\s{0,3}(`{3,}|~{3,})\s*$/;

// Recognizes an opening CommonMark fence without an unbounded marker-and-tail
// expression, preserving the existing indentation, marker, and info rules.
function openingFenceMarker(line) {
  let indentationLength = 0;
  while (indentationLength < 3 && /\s/.test(line[indentationLength] ?? "")) {
    indentationLength += 1;
  }
  const char = line[indentationLength];
  if (char !== "`" && char !== "~") return null;
  let markerEnd = indentationLength + 1;
  while (line[markerEnd] === char) markerEnd += 1;
  if (markerEnd - indentationLength < 3) return null;
  return {
    char,
    length: markerEnd - indentationLength,
    info: line.slice(markerEnd),
  };
}

// Determines whether a marker-bearing line closes the given open fence: same
// marker character, at least as long, and nothing else on the line.
function closesFence(marker, fence, line) {
  return marker !== null
    && marker.char === fence.char
    && marker.length >= fence.length
    && CLOSING_FENCE_PATTERN.test(line);
}

// Consumes one line inside an open fence: a closing marker closes the block
// (pushing accumulated Mermaid content when the fence carried the `mermaid`
// info string), and any other line accumulates into the current block.
function consumeFencedLine(line, marker, fence, current, blocks) {
  if (closesFence(marker, fence, line)) {
    if (current !== null) blocks.push(current.join("\n"));
    return { fence: null, current: null };
  }
  if (current !== null) current.push(line);
  return { fence, current };
}

// Collects only real fenced Mermaid blocks: a fence opens on a triple-plus
// backtick or tilde run whose info string is exactly `mermaid` and closes
// only on a same-character run at least as long (CommonMark). A literal
// ```mermaid example nested inside an outer longer fence is content of that
// outer fence, never a diagram, so it can neither satisfy a diagram gate
// nor be syntax-checked.
function mermaidBlocks(text) {
  const lines = text.split("\n");
  let fence = null;
  let current = null;
  const blocks = [];
  for (const line of lines) {
    const marker = openingFenceMarker(line);
    if (fence !== null) {
      ({ fence, current } = consumeFencedLine(line, marker, fence, current, blocks));
      continue;
    }
    if (marker) {
      fence = { char: marker.char, length: marker.length };
      current = marker.info.trim() === "mermaid" ? [] : null;
    }
  }
  return blocks;
}

// Builds the Mermaid syntax and ADD diagram-completeness validator from the
// configured parser and shared Markdown helpers.
export function createMermaidValidator(context) {
  const {
    escapeRegExp,
    mermaid,
    metadata,
    sectionContent,
    stripFencedCode,
    tableFromContent,
  } = context;

  function tableIds(content, pattern) {
    const table = tableFromContent(content);
    const idColumn = table?.header.indexOf("ID") ?? -1;
    if (idColumn < 0) return [];
    return table.rows.map((row) => row[idColumn]).filter((id) => pattern.test(id));
  }

  // A conditional flow section is exempt only when its entire stripped body
  // is one reasoned N/A: a real table followed by a stray N/A line still owes
  // its triggered table/diagram gates.
  function wholeBodyIsNa(content) {
    const body = stripFencedCode(content).trim();
    return body.length > 0 && !body.includes("\n") && /^N\/A\s+—\s+\S/.test(body);
  }

  // Whether this file is a Current ADD, whose triggered design gates apply
  // (Draft ADDs may be temporarily incomplete).
  function isCurrentAdd(path, markdown) {
    return path.includes("/architecture/")
      && path.split("/").at(-1).startsWith("ADD-")
      && metadata(markdown, "Design Status") === "Current";
  }

  // Concatenates a section's Mermaid diagrams with `%%` comment lines removed:
  // a Mermaid comment renders nothing, so an ID mentioned only in a comment
  // does not cover its flow or interaction.
  function commentStrippedDiagrams(content) {
    return mermaidBlocks(content)
      .map((block) => block.split("\n").filter((line) => !line.trimStart().startsWith("%%")).join("\n"))
      .join("\n");
  }

  async function validateMermaid(path, markdown, errors) {
    await validateMermaidSyntax(path, markdown, errors);
    validateArchitectureDesignCompleteness(path, markdown, errors);
    validateDiagramIdCoverage(path, markdown, errors);
    validateCurrentAddFlowGates(path, markdown, errors);
  }

  // Syntax-checks every Mermaid block through the configured parser.
  async function validateMermaidSyntax(path, markdown, errors) {
    for (const [index, block] of mermaidBlocks(markdown).entries()) {
      try {
        const result = await mermaid.parse(block, { suppressErrors: true });
        if (result === false) throw new Error("parser rejected the diagram");
      } catch (error) {
        errors.push(`${path}: invalid Mermaid block ${index + 1}: ${error.message}`);
      }
    }
  }

  // A Current ADD's Architecture Design must contain its own structured
  // component table and a Mermaid flowchart in this section (an unauthorized
  // N/A cannot substitute for a Required section).
  function validateArchitectureDesignCompleteness(path, markdown, errors) {
    if (!isCurrentAdd(path, markdown)) return;
    const content = sectionContent(markdown, "Architecture Design");
    if (!content) return;
    if (tableIds(content, /^C-\d+$/).length === 0) {
      errors.push(`${path}: Architecture Design requires a structured component table`);
    }
    const sectionDiagrams = mermaidBlocks(content);
    if (sectionDiagrams.length === 0) {
      errors.push(`${path}: Architecture Design requires a Mermaid flowchart in this section`);
      return;
    }
    if (!sectionDiagrams.some((d) => /^flowchart\b/m.test(d))) {
      errors.push(`${path}: Architecture Design Mermaid diagram must be a flowchart`);
    }
  }

  // Every table-declared component, flow, or interaction ID must appear in
  // its section's Mermaid diagrams.
  function validateDiagramIdCoverage(path, markdown, errors) {
    for (const [section, idPattern, description] of [
      ["Architecture Design", /^C-\d+$/, "Mermaid architecture diagram"],
      ["Control Flow Design", /^CF-\d+$/, "Mermaid control-flow diagram"],
      ["Interaction Flow Design", /^IX-\d+$/, "Mermaid interaction-flow diagram"],
    ]) {
      const content = sectionContent(markdown, section);
      if (!content || wholeBodyIsNa(content)) continue;
      const diagrams = commentStrippedDiagrams(content);
      for (const id of tableIds(content, idPattern)) {
        if (!new RegExp(`(^|[^A-Za-z0-9-])${escapeRegExp(id)}($|[^A-Za-z0-9-])`).test(diagrams)) {
          errors.push(`${path}: ${description} does not cover ${id}`);
        }
      }
    }
  }

  // A Current ADD's triggered Control Flow and Interaction Flow sections must
  // each carry a structured table, a Mermaid diagram, and an allowed diagram
  // type (Draft ADDs may be incomplete). Control Flow allows a flowchart or
  // sequenceDiagram; Interaction Flow allows a sequenceDiagram or stateDiagram-v2.
  function validateCurrentAddFlowGates(path, markdown, errors) {
    if (!isCurrentAdd(path, markdown)) return;
    for (const [section, idPattern, allowedTypes] of [
      ["Control Flow Design", /^CF-\d+$/, [/^flowchart\b/m, /^sequenceDiagram\b/m]],
      ["Interaction Flow Design", /^IX-\d+$/, [/^sequenceDiagram\b/m, /^stateDiagram-v2\b/m]],
    ]) {
      const content = sectionContent(markdown, section);
      if (!content || wholeBodyIsNa(content)) continue;
      const ids = tableIds(content, idPattern);
      const diagrams = mermaidBlocks(content);
      if (ids.length === 0) {
        errors.push(`${path}: ${section} requires a structured table for a Current ADD`);
      }
      if (diagrams.length === 0) {
        errors.push(`${path}: ${section} requires a Mermaid diagram for a Current ADD`);
      } else if (!diagrams.some((d) => allowedTypes.some((type) => type.test(d)))) {
        errors.push(`${path}: ${section} Mermaid diagram must be an allowed type for a Current ADD`);
      }
    }
  }

  return { validateMermaid };
}
