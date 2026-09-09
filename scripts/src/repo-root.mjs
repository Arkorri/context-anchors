// Locates the repository root by looking for the workspace manifest rather than by counting
// `..` segments, so moving a script does not silently redirect the paths it reads and writes.
// @noref[scripts/src/repo-root.mjs]

import { existsSync } from "node:fs";
import { dirname, join, parse } from "node:path";
import { fileURLToPath } from "node:url";

const MARKER = "Cargo.toml";

/// Walks up from `start` to the first directory holding the workspace manifest.
export function repoRootFrom(start) {
  let directory = start;
  for (;;) {
    if (existsSync(join(directory, MARKER))) return directory;
    const parent = dirname(directory);
    if (parent === directory || parent === parse(directory).root) {
      throw new Error(`no ${MARKER} above ${start}; not inside the repository`);
    }
    directory = parent;
  }
}

export function repoRoot(importMetaUrl) {
  return repoRootFrom(dirname(fileURLToPath(importMetaUrl)));
}
