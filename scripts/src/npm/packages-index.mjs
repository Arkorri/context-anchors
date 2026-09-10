// The contract between the three jobs in @noref[.github/workflows/publish-npm.yml]: `build` packs
// the tarballs and writes `packages.json` beside them, `verify` and `publish` read it back.
//
// Tarballs rather than a directory tree because `actions/upload-artifact` zips its payload, and zip
// does not carry the executable bit. The platform binary is not a declared `bin` entry — the shim
// reaches it through `require.resolve` — so npm never chmods it on install, and a mode lost in
// transit would reach users as EACCES. Modes inside a tarball are opaque to the zip.

import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { join } from "node:path";

export const INDEX_FILE = "packages.json";

export function digestOf(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

// Stored POSIX-relative so the index survives the trip to a Windows runner.
export function tarballPath(dir, entry) {
  return join(dir, ...entry.tarball.split("/"));
}

function invalid(message) {
  throw new Error(`${INDEX_FILE}: ${message}`);
}

export function validateIndex(index, { expectVersion } = {}) {
  if (!index || typeof index !== "object") invalid("not an object");
  if (typeof index.version !== "string" || index.version === "") invalid("missing version");
  if (!Array.isArray(index.packages) || index.packages.length === 0) invalid("no packages");

  const names = new Set();
  for (const entry of index.packages) {
    if (typeof entry?.name !== "string" || entry.name === "") invalid("entry without a name");
    if (names.has(entry.name)) invalid(`duplicate entry for ${entry.name}`);
    names.add(entry.name);
    if (typeof entry.tarball !== "string" || entry.tarball === "") {
      invalid(`${entry.name} has no tarball`);
    }
    if (entry.tarball.startsWith("/") || entry.tarball.split("/").includes("..")) {
      invalid(`${entry.name} tarball path must be relative and contained: ${entry.tarball}`);
    }
    if (!/^[0-9a-f]{64}$/.test(entry.sha256 ?? "")) invalid(`${entry.name} has no sha256`);
    if (entry.shim !== true && (typeof entry.platform !== "string" || entry.platform === "")) {
      invalid(`${entry.name} is neither the shim nor a platform package`);
    }
  }

  const shims = index.packages.filter((entry) => entry.shim === true);
  if (shims.length !== 1) invalid(`expected exactly one shim, found ${shims.length}`);

  if (expectVersion !== undefined && index.version !== expectVersion) {
    invalid(`version is ${index.version}, expected ${expectVersion}`);
  }
  return index;
}

export function readIndex(dir, options) {
  return validateIndex(JSON.parse(readFileSync(join(dir, INDEX_FILE), "utf8")), options);
}

// Reading the bytes back and comparing against the index is what makes "verify tested the same
// bytes that get published" a fact rather than an assumption.
export function readVerifiedTarball(dir, entry) {
  const path = tarballPath(dir, entry);
  const bytes = readFileSync(path);
  const actual = digestOf(bytes);
  if (actual !== entry.sha256) {
    throw new Error(`${entry.name}: sha256 mismatch\n  expected ${entry.sha256}\n  actual   ${actual}`);
  }
  return path;
}

// npm is `npm.cmd` on Windows, which cannot be launched directly by spawn/execFile; `cmd.exe /c` is
// the supported form. `shell: true` would also work but is deprecated under DEP0190 and pushes the
// quoting onto us.
export function npmInvocation(platform, args) {
  return platform === "win32"
    ? { file: "cmd.exe", args: ["/c", "npm", ...args] }
    : { file: "npm", args };
}

// npm treats a bare `foo/bar` argument as a GitHub shorthand, not a path — the bug that stopped the
// shim publishing at 0.0.1. Every spec we hand npm goes through here first.
export function assertPathSpec(spec) {
  if (typeof spec !== "string" || spec === "") throw new Error("empty package spec");
  const absolute = spec.startsWith("/") || /^[A-Za-z]:[\\/]/.test(spec);
  if (!absolute) {
    throw new Error(
      `package spec must be absolute, got ${spec} — npm would resolve a bare owner/repo as a git shorthand`,
    );
  }
  return spec;
}
