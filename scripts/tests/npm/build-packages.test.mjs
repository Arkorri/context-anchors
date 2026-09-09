// @noref[scripts/src/npm/build-packages.mjs, bin/anchr.js]
import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";

import {
  PLATFORMS,
  findArchiveName,
  findFile,
  platformPackageJson,
  readme,
  shimPackageJson,
  shimSource,
  versionOf,
} from "../../src/npm/build-packages.mjs";

const manifest = {
  releases: [{ app_name: "context-anchors", app_version: "0.0.1" }],
  artifacts: {
    "context-anchors-aarch64-apple-darwin.tar.xz": {
      kind: "executable-zip",
      target_triples: ["aarch64-apple-darwin"],
    },
    "context-anchors-installer.sh": { kind: "installer", target_triples: [] },
  },
};

test("the version comes from the shim's own release entry", () => {
  assert.equal(versionOf(manifest), "0.0.1");
  assert.throws(() => versionOf({ releases: [{ app_name: "other" }] }), /no release/);
  assert.throws(() => versionOf({}), /no release/);
});

test("only executable archives match a target triple", () => {
  assert.equal(
    findArchiveName(manifest, "aarch64-apple-darwin"),
    "context-anchors-aarch64-apple-darwin.tar.xz",
  );
  assert.equal(findArchiveName(manifest, "x86_64-pc-windows-msvc"), null);
  assert.equal(findArchiveName({}, "aarch64-apple-darwin"), null);
});

// The bug that shipped 0.0.1's tarballs without licence text: `files` is an allowlist, so a
// licence not named here is simply absent from the published package.
test("every platform package ships both licence files", () => {
  for (const platform of Object.values(PLATFORMS)) {
    const json = platformPackageJson("0.0.1", platform);
    assert.deepEqual(json.files, ["bin", "LICENSE-MIT", "LICENSE-APACHE"]);
    assert.equal(json.license, "MIT OR Apache-2.0");
  }
});

test("a platform package is installable on exactly one os and cpu", () => {
  const json = platformPackageJson("0.0.1", PLATFORMS["x86_64-pc-windows-msvc"]);
  assert.equal(json.name, "@context-anchors/win32-x64");
  assert.deepEqual(json.os, ["win32"]);
  assert.deepEqual(json.cpu, ["x64"]);
});

test("every platform in the table has a distinct package name", () => {
  const names = Object.values(PLATFORMS).map((p) => platformPackageJson("0.0.1", p).name);
  assert.equal(new Set(names).size, names.length, names.join(", "));
});

// A wrong optionalDependencies map is the failure mode that breaks installs for real users.
test("the shim optionally depends on exactly the platforms that were built", () => {
  const built = ["@context-anchors/darwin-arm64", "@context-anchors/linux-x64"];
  const json = shimPackageJson("0.0.1", built);
  assert.deepEqual(Object.keys(json.optionalDependencies), built);
  for (const version of Object.values(json.optionalDependencies)) {
    assert.equal(version, "0.0.1", "platform packages must be pinned to the shim's version");
  }
  assert.deepEqual(json.bin, { anchr: "bin/anchr.js" });
  assert.deepEqual(json.files, ["bin", "README.md", "LICENSE-MIT", "LICENSE-APACHE"]);
});

test("a shim built with no platform packages declares none", () => {
  assert.deepEqual(shimPackageJson("0.0.1", []).optionalDependencies, {});
});

test("the shim source is valid javascript that names the scope and binary", () => {
  const source = shimSource();
  // Node strips the shebang when it loads a file; `new Function` does not, so drop it first.
  const body = source.replace(/^#![^\n]*\n/, "");
  assert.doesNotThrow(() => new Function(body));
  assert.match(source, /@context-anchors\/\$\{process\.platform\}-\$\{process\.arch\}/);
  assert.match(source, /anchr\.exe/);
  assert.match(source, /process\.exit\(2\)/);
});

test("the readme points at the repository rather than a bare package name", () => {
  assert.match(readme(), /github\.com\/Arkorri\/context-anchors/);
});

test("findFile locates a nested match and reports absence as null", () => {
  const base = mkdtempSync(join(tmpdir(), "anchr-find-"));
  mkdirSync(join(base, "a", "b"), { recursive: true });
  writeFileSync(join(base, "a", "b", "anchr"), "binary");

  assert.equal(findFile(base, "anchr"), join(base, "a", "b", "anchr"));
  assert.equal(findFile(base, "missing"), null);
});
