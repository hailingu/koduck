// ADR: docs/adr/ADR-0011-metadata-entry-recognition-reliability.md

/**
 * Builds the active Metadata reader and duplicate-field validator using the
 * caller's canonical fenced-code and section parsers.
 */
export function createMetadataValidator({ stripFencedCode, sectionContent }) {
  // Parses one active Metadata list entry without an unbounded whole-line
  // expression, preserving the established marker, delimiter, and trimming
  // rules for the extracted field/value pair.
  function metadataEntry(line) {
    const prefix = "- **";
    if (!line.startsWith(prefix)) return undefined;
    const labelEnd = line.indexOf("**:", prefix.length);
    if (labelEnd <= prefix.length) return undefined;
    return {
      field: line.slice(prefix.length, labelEnd).replace(/\s+\[[^\]]+\]\s*$/, "").trim(),
      value: line.slice(labelEnd + 3).trim(),
    };
  }

  // Collects active fields only from the real Metadata section; historical or
  // narrative lookalikes outside that section remain excluded.
  function entries(markdown) {
    const content = sectionContent(stripFencedCode(markdown), "Metadata") ?? "";
    const result = [];
    for (const line of content.split("\n")) {
      const entry = metadataEntry(line);
      if (entry !== undefined) result.push(entry);
    }
    return result;
  }

  // Returns only the sole active value; duplicates never become canonical
  // input to lifecycle or approval checks.
  function metadata(markdown, field) {
    const values = entries(markdown)
      .filter((entry) => entry.field === field)
      .map((entry) => entry.value);
    return values.length === 1 ? values[0] : undefined;
  }

  // Rejects every repeated active field, including optional lifecycle fields.
  function validateUniqueMetadata(path, markdown, errors) {
    const counts = new Map();
    for (const { field } of entries(markdown)) {
      counts.set(field, (counts.get(field) ?? 0) + 1);
    }
    for (const [field, count] of counts) {
      if (count > 1) {
        errors.push(`${path}: metadata field ${field} must appear exactly once; found ${count}`);
      }
    }
  }

  return { metadata, validateUniqueMetadata };
}
