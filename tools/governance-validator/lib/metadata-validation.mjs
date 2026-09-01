// ADR: docs/adr/ADR-0012-metadata-helper-scope-maintainability.md

// Removes a trailing requirement-level suffix by its bounded delimiters while
// preserving the original whitespace and bracket-content constraints.
function fieldWithoutRequirementLevelSuffix(field) {
  let end = field.length;
  while (end > 0 && /\s/.test(field[end - 1])) end -= 1;
  if (end === 0 || field[end - 1] !== "]") return field.trim();

  let suffixStart = end - 2;
  while (suffixStart >= 0) {
    if (field[suffixStart] === "]") return field.trim();
    if (field[suffixStart] === "[" && suffixStart > 0 && /\s/.test(field[suffixStart - 1])) break;
    suffixStart -= 1;
  }
  if (suffixStart < 0 || suffixStart === end - 2) return field.trim();

  let labelEnd = suffixStart;
  while (labelEnd > 0 && /\s/.test(field[labelEnd - 1])) labelEnd -= 1;
  return field.slice(0, labelEnd);
}

/**
 * Parses one active Metadata list entry without an unbounded whole-line
 * expression, preserving the established marker, delimiter, and trimming
 * rules for the extracted field/value pair.
 */
function metadataEntry(line) {
  const prefix = "- **";
  if (!line.startsWith(prefix)) return undefined;
  const labelEnd = line.indexOf("**:", prefix.length);
  if (labelEnd <= prefix.length) return undefined;
  return {
    field: fieldWithoutRequirementLevelSuffix(line.slice(prefix.length, labelEnd)),
    value: line.slice(labelEnd + 3).trim(),
  };
}

/**
 * Builds the active Metadata reader and duplicate-field validator using the
 * caller's canonical fenced-code and section parsers.
 */
export function createMetadataValidator({ stripFencedCode, sectionContent }) {
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
