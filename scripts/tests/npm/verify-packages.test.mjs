// @noref[scripts/src/npm/verify-packages.mjs, bin/anchr.js]
import assert from "node:assert/strict";
import { test } from "node:test";

import {
  binShimPath,
  binaryName,
  fixtureFiles,
  hostPlatform,
  installArgs,
  selectPackages,
  versionMatches,
} from "../../src/npm/verify-packages.mjs";

const index = {
  version: "0.0.2",
  packages: [
    { name: "@context-anchors/darwin-arm64", platform: "darwin-arm64", tarball: "tarballs/a.tgz" },
    { name: "context-anchors", shim: true, tarball: "tarballs/b.tgz" },
  ],
};

// Guards a runner label quietly changing architecture underneath us, which would otherwise
// "verify" a package that never ran.
test("the host maps to exactly one platform package, or is refused", () => {
  assert.equal(hostPlatform("darwin", "arm64"), "darwin-arm64");
  assert.equal(hostPlatform("win32", "x64"), "win32-x64");
  assert.equal(hostPlatform("linux", "arm64"), "linux-arm64");
  assert.throws(() => hostPlatform("linux", "s390x"), /no platform package covers this host/);
  assert.throws(() => hostPlatform("freebsd", "x64"), /no platform package covers this host/);
});

test("only Windows carries the .exe suffix", () => {
  assert.equal(binaryName("win32-x64"), "anchr.exe");
  assert.equal(binaryName("darwin-arm64"), "anchr");
  assert.equal(binaryName("linux-x64"), "anchr");
});

// npm writes three shims; only the .cmd is runnable by CreateProcess.
test("the .bin shim is the .cmd on Windows", () => {
  assert.ok(binShimPath("node_modules", "win32-x64").endsWith("anchr.cmd"));
  assert.ok(binShimPath("node_modules", "linux-x64").endsWith("anchr"));
  assert.ok(!binShimPath("node_modules", "linux-x64").endsWith(".cmd"));
});

test("installs never run scripts, and every spec is absolute", () => {
  const args = installArgs(["/abs/a.tgz", "/abs/b.tgz"]);
  assert.ok(args.includes("--ignore-scripts"));
  assert.deepEqual(args.slice(-2), ["/abs/a.tgz", "/abs/b.tgz"]);
  assert.ok(!args.includes("--omit=optional"));
  assert.throws(() => installArgs(["npm-dist/context-anchors"]), /must be absolute/);
});

// Without --omit=optional the degradation check stops testing anything the moment the platform
// package exists on the registry, because npm would satisfy the optional dependency from there.
test("the degradation check omits optional dependencies", () => {
  assert.ok(installArgs(["/abs/b.tgz"], { omitOptional: true }).includes("--omit=optional"));
});

test("version output is matched exactly, tolerating Windows line endings", () => {
  assert.ok(versionMatches("anchr 0.0.2\n", "0.0.2"));
  assert.ok(versionMatches("anchr 0.0.2\r\n", "0.0.2"));
  assert.ok(versionMatches("  anchr 0.0.2  ", "0.0.2"));
  assert.ok(!versionMatches("anchr 0.0.20\n", "0.0.2"));
  assert.ok(!versionMatches("anchr 0.0.1\n", "0.0.2"));
  assert.ok(!versionMatches("", "0.0.2"));
});

test("selecting a platform yields it and the shim, or explains what is missing", () => {
  const { shim, platform } = selectPackages(index, "darwin-arm64");
  assert.equal(shim.name, "context-anchors");
  assert.equal(platform.name, "@context-anchors/darwin-arm64");
  assert.throws(() => selectPackages(index, "win32-x64"), /names no package for win32-x64/);
  assert.throws(
    () => selectPackages({ packages: [index.packages[0]] }, "darwin-arm64"),
    /names no shim/,
  );
});

// The fixture has to actually make the binary do work and fail, or the exit-code plumbing through
// the shim is never exercised.
test("the broken fixture carries an unresolvable reference and the clean one does not", () => {
  const broken = Object.fromEntries(fixtureFiles("broken").map((f) => [f.path, f.contents]));
  const clean = Object.fromEntries(fixtureFiles("clean").map((f) => [f.path, f.contents]));
  assert.match(broken["anchr.toml"], /\[root\]/);
  assert.match(broken["doc.md"], /@ref\[#missing\/thing\]/);
  assert.doesNotMatch(clean["doc.md"], /missing\/thing/);
  assert.match(clean["doc.md"], /@anchor\[real\/thing\]/);
  assert.match(clean["doc.md"], /@ref\[#real\/thing\]/);
});
