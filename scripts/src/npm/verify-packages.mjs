#!/usr/bin/env node
// Gates publishing. Runs on one runner per platform package and exercises the packed tarballs the
// way a registry install would, before any version is consumed.
//
//   node scripts/src/npm/verify-packages.mjs --dir npm-dist --platform darwin-arm64 \
//        --expect-version 0.0.2 [--keep]
//
// Installing a directory (`npm install ../@context-anchors/linux-x64`) would symlink it, and Node
// then resolves the shim from the link's realpath — so the check passes even with a wrong `files`
// allowlist or a lost executable bit. Installing the tarball copies real files through npm's real
// extract path.
//
// Exit 0 verified, 1 an assertion failed, 2 a contract error (bad flags, bad index, wrong host).

import { spawnSync } from "node:child_process";
import { existsSync, mkdtempSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";

import {
  assertPathSpec,
  npmInvocation,
  readIndex,
  readVerifiedTarball,
} from "./packages-index.mjs";

const SCOPE = "@context-anchors";
const SHIM_NAME = "context-anchors";
const BIN = "anchr";

const HOSTS = new Set(["darwin-arm64", "darwin-x64", "linux-arm64", "linux-x64", "win32-x64"]);

function isWindows(platform) {
  return platform.startsWith("win32");
}

export function hostPlatform(platform, arch) {
  const host = `${platform}-${arch}`;
  if (!HOSTS.has(host)) throw new Error(`no platform package covers this host: ${host}`);
  return host;
}

export function binaryName(platform) {
  return isWindows(platform) ? `${BIN}.exe` : BIN;
}

// npm writes three shims; only the .cmd is runnable by CreateProcess on Windows.
export function binShimPath(nodeModulesDir, platform) {
  return join(nodeModulesDir, ".bin", isWindows(platform) ? `${BIN}.cmd` : BIN);
}

export function installArgs(tarballs, { omitOptional = false } = {}) {
  const args = ["install", "--ignore-scripts", "--no-audit", "--no-fund", "--loglevel=error"];
  if (omitOptional) args.push("--omit=optional");
  return [...args, ...tarballs.map(assertPathSpec)];
}

export function versionMatches(stdout, version) {
  return stdout.trim() === `${BIN} ${version}`;
}

export function selectPackages(index, platform) {
  const shim = index.packages.find((entry) => entry.shim === true);
  const target = index.packages.find((entry) => entry.platform === platform);
  if (!shim) throw new Error("index names no shim package");
  if (!target) throw new Error(`index names no package for ${platform}`);
  return { shim, platform: target };
}

export function fixtureFiles(kind) {
  const resolves = "# Doc\n\n<!-- @anchor[real/thing] -->\n\nSee @ref[#real/thing].\n";
  return [
    { path: "anchr.toml", contents: '[root]\nname = "verify"\n' },
    {
      path: "doc.md",
      contents: kind === "broken" ? `${resolves}\nAlso @ref[#missing/thing].\n` : resolves,
    },
  ];
}

function run(file, args, options = {}) {
  return spawnSync(file, args, { encoding: "utf8", ...options });
}

function runNpm(args, options) {
  const { file, args: argv } = npmInvocation(process.platform, args);
  const result = run(file, argv, options);
  if (result.status !== 0) {
    throw new Error(`npm ${args[0]} failed (${result.status}):\n${result.stderr ?? ""}`);
  }
  return result;
}

function runShim(shimPath, args, options) {
  return isWindows(process.platform)
    ? run("cmd.exe", ["/c", shimPath, ...args], options)
    : run(shimPath, args, options);
}

function scratchDir() {
  const dir = mkdtempSync(join(tmpdir(), "anchr-verify-"));
  writeFileSync(
    join(dir, "package.json"),
    `${JSON.stringify({ name: "anchr-verify", version: "0.0.0", private: true }, null, 2)}\n`,
  );
  return dir;
}

function writeFixture(dir, kind) {
  for (const file of fixtureFiles(kind)) writeFileSync(join(dir, file.path), file.contents);
}

// npm records where each package came from; a `file:` origin proves the installed bytes are the
// ones we packed rather than a same-numbered copy already on the registry.
function installedFrom(scratch, packageName) {
  const lockPath = join(scratch, "node_modules", ".package-lock.json");
  if (!existsSync(lockPath)) return "";
  const lock = JSON.parse(readFileSync(lockPath, "utf8"));
  return lock.packages?.[`node_modules/${packageName}`]?.resolved ?? "";
}

function verify({ scratch, platform, version, shimTarball, platformTarball, failures }) {
  runNpm(installArgs([shimTarball, platformTarball]), { cwd: scratch });

  const nodeModules = join(scratch, "node_modules");
  const platformDir = join(nodeModules, SCOPE, platform);
  const binary = join(platformDir, "bin", binaryName(platform));

  const record = (condition, message) => {
    if (!condition) failures.push(message);
  };

  record(existsSync(binary), `binary missing: ${binary}`);
  if (existsSync(binary) && !isWindows(platform)) {
    record((statSync(binary).mode & 0o111) !== 0, `binary is not executable: ${binary}`);
  }
  for (const licence of ["LICENSE-MIT", "LICENSE-APACHE"]) {
    record(existsSync(join(platformDir, licence)), `${platform} package is missing ${licence}`);
  }
  record(existsSync(join(nodeModules, SHIM_NAME, "README.md")), "shim is missing README.md");

  const origin = installedFrom(scratch, `${SCOPE}/${platform}`);
  record(
    origin.startsWith("file:"),
    `${SCOPE}/${platform} resolved to ${origin || "nothing"}, not the packed tarball`,
  );

  const installedVersion = existsSync(join(platformDir, "package.json"))
    ? JSON.parse(readFileSync(join(platformDir, "package.json"), "utf8")).version
    : "<absent>";
  record(installedVersion === version, `${platform} package is ${installedVersion}, want ${version}`);

  const direct = run(process.execPath, [join(nodeModules, SHIM_NAME, "bin", `${BIN}.js`), "--version"]);
  record(
    direct.status === 0 && versionMatches(direct.stdout ?? "", version),
    `bin/${BIN}.js --version gave (${direct.status}) ${JSON.stringify(direct.stdout)}`,
  );

  const shimPath = binShimPath(nodeModules, platform);
  const viaBin = runShim(shimPath, ["--version"]);
  record(
    viaBin.status === 0 && versionMatches(viaBin.stdout ?? "", version),
    `.bin shim --version gave (${viaBin.status}) ${JSON.stringify(viaBin.stdout)}`,
  );

  // The binary has to actually do work, and the shim has to carry the exit code back out.
  writeFixture(scratch, "broken");
  const broken = runShim(shimPath, ["check", "--color", "never"], { cwd: scratch });
  record(broken.status === 1, `check on a broken reference exited ${broken.status}, want 1`);

  writeFixture(scratch, "clean");
  const clean = runShim(shimPath, ["check", "--color", "never"], { cwd: scratch });
  record(clean.status === 0, `check on a clean root exited ${clean.status}, want 0`);
}

// Without --omit=optional this stops being a test the moment the platform package exists on the
// registry, because npm would satisfy the optional dependency from there.
function verifyDegradation({ scratch, platform, shimTarball, failures }) {
  runNpm(installArgs([shimTarball], { omitOptional: true }), { cwd: scratch });

  const shimPath = binShimPath(join(scratch, "node_modules"), platform);
  const result = runShim(shimPath, ["--version"]);
  const stderr = result.stderr ?? "";
  if (result.status !== 2) {
    failures.push(`shim without its platform package exited ${result.status}, want 2`);
  }
  if (!stderr.includes(`${SCOPE}/${platform}`)) {
    failures.push(`shim's failure message does not name ${SCOPE}/${platform}: ${stderr.trim()}`);
  }
}

function main() {
  const { values: args } = parseArgs({
    options: {
      dir: { type: "string" },
      platform: { type: "string" },
      "expect-version": { type: "string" },
      keep: { type: "boolean", default: false },
    },
  });
  for (const required of ["dir", "platform", "expect-version"]) {
    if (!args[required]) {
      console.error(`missing --${required}`);
      process.exit(2);
    }
  }
  const version = args["expect-version"];

  let shimTarball;
  let platformTarball;
  try {
    const index = readIndex(args.dir, { expectVersion: version });
    const selected = selectPackages(index, args.platform);
    const host = hostPlatform(process.platform, process.arch);
    if (host !== args.platform) {
      throw new Error(`running on ${host} but asked to verify ${args.platform}`);
    }
    shimTarball = resolve(readVerifiedTarball(args.dir, selected.shim));
    platformTarball = resolve(readVerifiedTarball(args.dir, selected.platform));
  } catch (error) {
    console.error(error.message);
    process.exit(2);
  }

  const failures = [];
  const scratches = [scratchDir(), scratchDir()];
  try {
    verify({ scratch: scratches[0], platform: args.platform, version, shimTarball, platformTarball, failures });
    verifyDegradation({ scratch: scratches[1], platform: args.platform, shimTarball, failures });
  } catch (error) {
    failures.push(error.message);
  } finally {
    if (args.keep) {
      console.log(`kept ${scratches.join(" ")}`);
    } else {
      // A just-exited child can still hold a handle on Windows.
      for (const dir of scratches) rmSync(dir, { recursive: true, force: true, maxRetries: 3 });
    }
  }

  if (failures.length > 0) {
    console.error(`${args.platform}: ${failures.length} check(s) failed`);
    for (const failure of failures) console.error(`  - ${failure}`);
    process.exit(1);
  }
  console.log(`${args.platform}: verified ${SHIM_NAME}@${version} end to end`);
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main();
}
