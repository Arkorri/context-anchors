#!/usr/bin/env node
// Publishes the packed tarballs, platform packages first and the shim last.
//
//   node scripts/src/npm/publish-packages.mjs --dir npm-dist --expect-version 0.0.2 --provenance
//
// Two invariants carry the safety here.
//
// Shim last: `context-anchors` is the only package anyone installs, so while it is unpublished a
// partial run is invisible to users. Reaching it means the other five are already accounted for.
//
// Skip only on a trustworthy positive: publishing something already published is a harmless 403,
// but skipping something that is *not* published ships a shim whose optionalDependency 404s, which
// npm swallows silently and which no later run would notice. So every ambiguous probe publishes.
//
// Exit 0 everything published or already present, 1 a publish failed, 2 a contract error.

import { spawnSync } from "node:child_process";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";

import {
  assertPathSpec,
  npmInvocation,
  readIndex,
  readVerifiedTarball,
} from "./packages-index.mjs";

const DEFAULT_REGISTRY = "https://registry.npmjs.org";
const PROBE_TIMEOUT_MS = 10_000;

export function publishOrder(index) {
  const platforms = index.packages.filter((entry) => entry.shim !== true);
  const shims = index.packages.filter((entry) => entry.shim === true);
  if (shims.length !== 1) throw new Error(`expected exactly one shim, found ${shims.length}`);
  if (platforms.length === 0) throw new Error("no platform packages to publish");
  return [...platforms, shims[0]];
}

// A prerelease must not take `latest`, or `npm install context-anchors` starts handing people an rc.
export function distTag(version) {
  return version.includes("-") ? "next" : "latest";
}

export function packumentUrl(registry, name, version) {
  return `${registry.replace(/\/+$/, "")}/${name.replace("/", "%2F")}/${version}`;
}

export function publishArgs(spec, { provenance = false, dryRun = false, registry, tag } = {}) {
  if (!tag) throw new Error("publish needs an explicit dist-tag");
  const args = ["publish", assertPathSpec(spec), "--access", "public", "--tag", tag];
  if (provenance) args.push("--provenance");
  if (dryRun) args.push("--dry-run");
  if (registry) args.push(`--registry=${registry}`);
  return args;
}

/// Only a self-consistent 200 counts as present; everything else is treated as absent so the
/// publish is attempted.
export function classifyProbe(status, body, version) {
  if (status === 200 && body?.version === version && body?.dist?.tarball) return "present";
  if (status === 404) return "absent";
  return "unknown";
}

/// npm surfaces a duplicate version and a permissions denial with the same `E403`, so the message
/// is the only discriminator. Anything unrecognised is a failure — treating a stray 403 as
/// success would silently drop a package the token cannot write.
export function classifyPublishResult(status, stderr = "") {
  if (status === 0) return "published";
  if (/cannot publish over the previously published version/i.test(stderr)) return "already-present";
  return "failed";
}

export function summarise(states) {
  const width = Math.max(...states.map((state) => state.name.length));
  const lines = states.map((state) => `  ${state.name.padEnd(width)}  ${state.state}`);
  const ok = states.every((state) => state.state === "published" || state.state === "already-present");
  return { ok, table: lines.join("\n") };
}

async function probe(registry, name, version) {
  try {
    const response = await fetch(packumentUrl(registry, name, version), {
      signal: AbortSignal.timeout(PROBE_TIMEOUT_MS),
      headers: { accept: "application/json" },
    });
    const body = response.status === 200 ? await response.json().catch(() => null) : null;
    return classifyProbe(response.status, body, version);
  } catch {
    return "unknown";
  }
}

function publish(tarball, options) {
  const { file, args } = npmInvocation(process.platform, publishArgs(tarball, options));
  const result = spawnSync(file, args, { encoding: "utf8", stdio: ["ignore", "inherit", "pipe"] });
  if (result.stderr) process.stderr.write(result.stderr);
  return classifyPublishResult(result.status, result.stderr ?? "");
}

async function main() {
  const { values: args } = parseArgs({
    options: {
      dir: { type: "string" },
      "expect-version": { type: "string" },
      provenance: { type: "boolean", default: false },
      "dry-run": { type: "boolean", default: false },
      registry: { type: "string", default: DEFAULT_REGISTRY },
    },
  });
  for (const required of ["dir", "expect-version"]) {
    if (!args[required]) {
      console.error(`missing --${required}`);
      process.exit(2);
    }
  }
  if (args.provenance && args["dry-run"]) {
    console.error("--provenance needs a real publish; it cannot be combined with --dry-run");
    process.exit(2);
  }

  const version = args["expect-version"];
  let ordered;
  try {
    const index = readIndex(args.dir, { expectVersion: version });
    ordered = publishOrder(index).map((entry) => ({
      ...entry,
      tarball: resolve(readVerifiedTarball(args.dir, entry)),
    }));
  } catch (error) {
    console.error(error.message);
    process.exit(2);
  }

  const tag = distTag(version);
  const states = ordered.map((entry) => ({ name: entry.name, state: "not attempted" }));
  let failed = false;

  for (const [position, entry] of ordered.entries()) {
    const found = await probe(args.registry, entry.name, version);
    if (found === "present") {
      states[position].state = "already-present";
      console.log(`already present: ${entry.name}@${version}`);
      continue;
    }
    const outcome = publish(entry.tarball, {
      provenance: args.provenance,
      dryRun: args["dry-run"],
      registry: args.registry,
      tag,
    });
    states[position].state = outcome;
    if (outcome === "failed") {
      failed = true;
      break;
    }
  }

  const { ok, table } = summarise(states);
  console.log(`\npublish order (shim last):\n${table}`);
  if (failed || !ok) {
    console.error("\npublish incomplete — nothing later in the order was attempted");
    process.exit(1);
  }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  await main();
}
