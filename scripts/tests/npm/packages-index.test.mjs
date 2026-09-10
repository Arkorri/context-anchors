// @noref[scripts/src/npm/packages-index.mjs, .github/workflows/publish-npm.yml]
import assert from "node:assert/strict";
import { test } from "node:test";

import {
  assertPathSpec,
  digestOf,
  npmInvocation,
  tarballPath,
  validateIndex,
} from "../../src/npm/packages-index.mjs";

function index(overrides = {}) {
  return {
    version: "0.0.2",
    packages: [
      {
        name: "@context-anchors/darwin-arm64",
        platform: "darwin-arm64",
        tarball: "tarballs/a.tgz",
        sha256: "a".repeat(64),
      },
      { name: "context-anchors", shim: true, tarball: "tarballs/b.tgz", sha256: "b".repeat(64) },
    ],
    ...overrides,
  };
}

test("a well-formed index validates, and the version can be cross-checked", () => {
  assert.equal(validateIndex(index()).version, "0.0.2");
  assert.equal(validateIndex(index(), { expectVersion: "0.0.2" }).version, "0.0.2");
  assert.throws(() => validateIndex(index(), { expectVersion: "0.0.3" }), /version is 0\.0\.2/);
});

test("an index without exactly one shim is rejected", () => {
  const none = index({ packages: [index().packages[0]] });
  assert.throws(() => validateIndex(none), /exactly one shim/);
  const two = index();
  two.packages.push({ ...two.packages[1], name: "other-shim" });
  assert.throws(() => validateIndex(two), /exactly one shim/);
});

test("entries need a name, a digest, and a platform unless they are the shim", () => {
  const noDigest = index();
  noDigest.packages[0].sha256 = "nope";
  assert.throws(() => validateIndex(noDigest), /no sha256/);

  const noPlatform = index();
  delete noPlatform.packages[0].platform;
  assert.throws(() => validateIndex(noPlatform), /neither the shim nor a platform package/);

  const duplicate = index();
  duplicate.packages[1] = { ...duplicate.packages[0] };
  assert.throws(() => validateIndex(duplicate), /duplicate entry/);
});

test("tarball paths must stay inside the index directory", () => {
  const escaping = index();
  escaping.packages[0].tarball = "../../etc/passwd";
  assert.throws(() => validateIndex(escaping), /relative and contained/);

  const absolute = index();
  absolute.packages[0].tarball = "/tmp/a.tgz";
  assert.throws(() => validateIndex(absolute), /relative and contained/);
});

test("POSIX-relative tarball paths rejoin under the host separator", () => {
  const joined = tarballPath("npm-dist", { tarball: "tarballs/a.tgz" });
  assert.ok(joined.endsWith("a.tgz"));
  assert.ok(!joined.includes("tarballs/tarballs"));
});

test("npm is reached through cmd.exe on Windows and directly elsewhere", () => {
  assert.deepEqual(npmInvocation("linux", ["pack"]), { file: "npm", args: ["pack"] });
  assert.deepEqual(npmInvocation("win32", ["pack"]), {
    file: "cmd.exe",
    args: ["/c", "npm", "pack"],
  });
});

// The bug that stopped the shim publishing at 0.0.1: npm read `npm-dist/context-anchors` as a
// GitHub shorthand and went looking for a git remote.
test("only an absolute spec is accepted, so npm can never read one as owner/repo", () => {
  assert.equal(assertPathSpec("/abs/npm-dist/context-anchors"), "/abs/npm-dist/context-anchors");
  assert.equal(assertPathSpec("C:\\npm-dist\\context-anchors"), "C:\\npm-dist\\context-anchors");
  assert.throws(() => assertPathSpec("npm-dist/context-anchors"), /must be absolute/);
  assert.throws(() => assertPathSpec("./npm-dist/context-anchors"), /must be absolute/);
  assert.throws(() => assertPathSpec(""), /empty package spec/);
});

test("digests are stable hex", () => {
  assert.match(digestOf(Buffer.from("anchr")), /^[0-9a-f]{64}$/);
  assert.equal(digestOf(Buffer.from("anchr")), digestOf(Buffer.from("anchr")));
  assert.notEqual(digestOf(Buffer.from("anchr")), digestOf(Buffer.from("anchor")));
});
