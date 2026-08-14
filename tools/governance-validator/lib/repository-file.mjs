import { existsSync, realpathSync, statSync } from "node:fs";
import { relative, resolve, sep } from "node:path";

// Converts a filesystem path to the repository's portable slash-separated form.
function repositoryPath(root, path) {
  return relative(root, path).split(sep).join("/");
}

/**
 * Resolves a document-supplied repository path to a canonical regular file.
 * Malformed links, directory targets, and symlink or lexical escapes become
 * validation errors rather than uncaught filesystem failures or external reads.
 */
export function resolveRepositoryFile(root, candidate, sourcePath, label, errors) {
  const absolute = resolve(root, candidate);
  const relativePath = repositoryPath(root, absolute);
  if (relativePath === ".." || relativePath.startsWith("../")) {
    errors.push(`${sourcePath}: ${label} escapes the repository root: ${candidate}`);
    return undefined;
  }
  if (!existsSync(absolute)) {
    errors.push(`${sourcePath}: ${label} does not exist: ${candidate}`);
    return undefined;
  }
  const canonicalPath = realpathSync(absolute);
  const canonicalRoot = realpathSync(root);
  const canonicalRelativePath = repositoryPath(canonicalRoot, canonicalPath);
  if (canonicalRelativePath === ".." || canonicalRelativePath.startsWith("../")) {
    errors.push(`${sourcePath}: ${label} resolves outside the repository root: ${candidate}`);
    return undefined;
  }
  if (!statSync(canonicalPath).isFile()) {
    errors.push(`${sourcePath}: ${label} is not a regular file: ${candidate}`);
    return undefined;
  }
  return canonicalPath;
}
