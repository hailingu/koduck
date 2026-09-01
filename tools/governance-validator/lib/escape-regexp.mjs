// Escapes regular-expression metacharacters so a literal field name or
// heading can be embedded safely in a constructed RegExp pattern.
export function escapeRegExp(value) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, String.raw`\$&`);
}
