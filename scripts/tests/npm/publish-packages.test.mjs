// @noref[scripts/src/npm/publish-packages.mjs]
import assert from "node:assert/strict";
import { test } from "node:test";

import {
  classifyProbe,
  classifyPublishResult,
  distTag,
  packumentUrl,
  publishArgs,
  publishOrder,
  summarise,
} from "../../src/npm/publish-packages.mjs";

const index = {
  version: "0.0.2",
  packages: [
    { name: "@context-anchors/darwin-arm64", platform: "darwin-arm64" },
    { name: "context-anchors", shim: true },
    { name: "@context-anchors/linux-x64", platform: "linux-x64" },
  ],
};

// The shim is the only package anyone installs, so while it is unpublished a partial run is
// invisible to users.
test("the shim publishes last whatever order the index is in", () => {
  const order = publishOrder(index).map((entry) => entry.name);
  assert.equal(order.at(-1), "context-anchors");
  assert.equal(order.length, 3);
  assert.throws(() => publishOrder({ packages: [{ name: "a", platform: "p" }] }), /exactly one shim/);
  assert.throws(() => publishOrder({ packages: [{ name: "s", shim: true }] }), /no platform packages/);
});

test("a prerelease never takes the latest tag", () => {
  assert.equal(distTag("0.0.2"), "latest");
  assert.equal(distTag("1.2.3"), "latest");
  assert.equal(distTag("0.0.2-rc.1"), "next");
  assert.equal(distTag("0.0.2-prerelease.1"), "next");
});

test("scoped names are escaped for the registry path, trailing slashes normalised", () => {
  assert.equal(
    packumentUrl("https://registry.npmjs.org", "@context-anchors/darwin-arm64", "0.0.2"),
    "https://registry.npmjs.org/@context-anchors%2Fdarwin-arm64/0.0.2",
  );
  assert.equal(
    packumentUrl("https://registry.npmjs.org/", "context-anchors", "0.0.2"),
    "https://registry.npmjs.org/context-anchors/0.0.2",
  );
});

test("publish arguments always carry an absolute spec, public access and an explicit tag", () => {
  const args = publishArgs("/abs/a.tgz", { tag: "latest", provenance: true });
  assert.deepEqual(args, ["publish", "/abs/a.tgz", "--access", "public", "--tag", "latest", "--provenance"]);
  assert.ok(!publishArgs("/abs/a.tgz", { tag: "next" }).includes("--provenance"));
  assert.ok(publishArgs("/abs/a.tgz", { tag: "next", dryRun: true }).includes("--dry-run"));
  assert.throws(() => publishArgs("npm-dist/context-anchors", { tag: "latest" }), /must be absolute/);
});

// Skipping something that is not actually published ships a shim whose optionalDependency 404s,
// which npm swallows silently — so only a self-consistent 200 may cause a skip.
test("only a self-consistent 200 counts as already present", () => {
  const body = { version: "0.0.2", dist: { tarball: "https://…/a.tgz" } };
  assert.equal(classifyProbe(200, body, "0.0.2"), "present");
  assert.equal(classifyProbe(404, null, "0.0.2"), "absent");
  assert.equal(classifyProbe(200, { version: "0.0.1", dist: { tarball: "x" } }, "0.0.2"), "unknown");
  assert.equal(classifyProbe(200, { version: "0.0.2" }, "0.0.2"), "unknown");
  assert.equal(classifyProbe(500, null, "0.0.2"), "unknown");
  assert.equal(classifyProbe(429, null, "0.0.2"), "unknown");
});

// npm reports a duplicate version and a permissions denial with the same E403, so the message is
// the only discriminator and an unrecognised 403 must not be mistaken for success.
test("a duplicate version is tolerated but a permissions denial is a failure", () => {
  assert.equal(classifyPublishResult(0, ""), "published");
  assert.equal(
    classifyPublishResult(1, "npm error 403 You cannot publish over the previously published versions: 0.0.2."),
    "already-present",
  );
  assert.equal(
    classifyPublishResult(1, "npm error 403 You do not have permission to publish context-anchors."),
    "failed",
  );
  assert.equal(
    classifyPublishResult(1, "npm error 403 Two-factor authentication or granular access token with bypass 2fa enabled is required"),
    "failed",
  );
  assert.equal(classifyPublishResult(1, "npm error network timeout"), "failed");
});

test("the summary reports success only when every package landed", () => {
  const done = [
    { name: "@context-anchors/linux-x64", state: "published" },
    { name: "context-anchors", state: "already-present" },
  ];
  assert.equal(summarise(done).ok, true);
  assert.match(summarise(done).table, /context-anchors\s+already-present/);
  assert.equal(summarise([...done, { name: "x", state: "not attempted" }]).ok, false);
  assert.equal(summarise([{ name: "x", state: "failed" }]).ok, false);
});
