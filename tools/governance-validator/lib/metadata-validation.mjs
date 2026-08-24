// ADR: docs/adr/ADR-0002-required-ai-ci-postgres-verification.md

/**
 * Builds the active Metadata reader and duplicate-field validator using the
 * caller's canonical fenced-code and section parsers.
 */
export function createMetadataValidator({ stripFencedCode, sectionContent }) {
  // Collects active fields only from the real Metadata section; historical or
  // narrative lookalikes outside that section remain excluded.
  function entries(markdown) {
    const content = sectionContent(stripFencedCode(markdown), "Metadata") ?? "";
    return [...content.matchAll(/^- \*\*(.+?)\*\*:\s*(.*)$/gm)].map((match) => ({
      field: match[1].replace(/\s+\[[^\]]+\]\s*$/, "").trim(),
      value: match[2].trim(),
    }));
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
