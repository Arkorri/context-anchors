// @noref[scripts/src/repo-root.mjs, Cargo.toml]
import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";

import { repoRoot, repoRootFrom } from "../src/repo-root.mjs";

test("the repo root is the directory holding the workspace manifest", () => {
  const base = mkdtempSync(join(tmpdir(), "anchr-root-"));
  writeFileSync(join(base, "Cargo.toml"), "[workspace]\n");
  const nested = join(base, "scripts", "src", "npm");
  mkdirSync(nested, { recursive: true });

  assert.equal(repoRootFrom(nested), base);
  assert.equal(repoRootFrom(base), base);
});

test("a directory outside any repository is an error, not a wrong answer", () => {
  const base = mkdtempSync(join(tmpdir(), "anchr-noroot-"));
  assert.throws(() => repoRootFrom(base), /no Cargo\.toml above/);
});

// The point of the helper: depth no longer decides the answer, so moving a script cannot
// silently redirect the files it reads and writes.
test("scripts at different depths resolve to the same root", () => {
  const fromScript = repoRoot(import.meta.url);
  const fromNested = repoRootFrom(join(fromScript, "scripts", "src", "npm"));
  assert.equal(fromNested, fromScript);
});
