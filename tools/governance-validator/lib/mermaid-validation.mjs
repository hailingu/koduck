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

  async function validateMermaid(path, markdown, errors) {
    const blocks = [...markdown.matchAll(/```mermaid\s*\n([\s\S]*?)```/g)];
    for (const [index, match] of blocks.entries()) {
      try {
        const result = await mermaid.parse(match[1], { suppressErrors: true });
        if (result === false) throw new Error("parser rejected the diagram");
      } catch (error) {
        errors.push(`${path}: invalid Mermaid block ${index + 1}: ${error.message}`);
      }
    }
    // Architecture Design completeness follows the ADD lifecycle: a Draft may be
    // temporarily incomplete, but a Current ADD must contain its own structured
    // component table and a Mermaid flowchart in this section (an unauthorized
    // N/A cannot substitute for a Required section).
    if (path.includes("/architecture/") && path.split("/").at(-1).startsWith("ADD-")
      && metadata(markdown, "Design Status") === "Current") {
      const content = sectionContent(markdown, "Architecture Design");
      if (content) {
        const sectionDiagrams = [...content.matchAll(/```mermaid\s*\n([\s\S]*?)```/g)].map((m) => m[1]);
        if (tableIds(content, /^C-\d+$/).length === 0) {
          errors.push(`${path}: Architecture Design requires a structured component table`);
        }
        if (sectionDiagrams.length === 0) {
          errors.push(`${path}: Architecture Design requires a Mermaid flowchart in this section`);
        } else if (!sectionDiagrams.some((d) => /^flowchart\b/m.test(d))) {
          errors.push(`${path}: Architecture Design Mermaid diagram must be a flowchart`);
        }
      }
    }
    
    for (const [section, idPattern, description] of [
      ["Architecture Design", /^C-\d+$/, "Mermaid architecture diagram"],
      ["Control Flow Design", /^CF-\d+$/, "Mermaid control-flow diagram"],
      ["Interaction Flow Design", /^IX-\d+$/, "Mermaid interaction-flow diagram"],
    ]) {
      const content = sectionContent(markdown, section);
      if (!content || /^\s*N\/A\s+—/m.test(content)) continue;
      const ids = tableIds(content, idPattern);
      const diagrams = [...content.matchAll(/```mermaid\s*\n([\s\S]*?)```/g)]
        .map((match) => match[1]).join("\n");
      for (const id of ids) {
        if (!new RegExp(`(^|[^A-Za-z0-9-])${escapeRegExp(id)}($|[^A-Za-z0-9-])`).test(diagrams)) {
          errors.push(`${path}: ${description} does not cover ${id}`);
        }
      }
    }
  
    // A Current ADD's triggered Control Flow and Interaction Flow sections must
    // each carry a structured table, a Mermaid diagram, and an allowed diagram
    // type (Draft ADDs may be incomplete). Control Flow allows a flowchart or
    // sequenceDiagram; Interaction Flow allows a sequenceDiagram or stateDiagram-v2.
    const isCurrentAdd = path.includes("/architecture/")
      && path.split("/").at(-1).startsWith("ADD-")
      && metadata(markdown, "Design Status") === "Current";
    if (isCurrentAdd) {
      for (const [section, idPattern, allowedTypes] of [
        ["Control Flow Design", /^CF-\d+$/, [/^flowchart\b/m, /^sequenceDiagram\b/m]],
        ["Interaction Flow Design", /^IX-\d+$/, [/^sequenceDiagram\b/m, /^stateDiagram-v2\b/m]],
      ]) {
        const content = sectionContent(markdown, section);
        if (!content || /^\s*N\/A\s+—/m.test(content)) continue;
        const ids = tableIds(content, idPattern);
        const diagrams = [...content.matchAll(/```mermaid\s*\n([\s\S]*?)```/g)].map((match) => match[1]);
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
  }
  
  
  return { validateMermaid };
}
