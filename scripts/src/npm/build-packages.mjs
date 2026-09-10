#!/usr/bin/env node
// Builds the npm packages for one release from cargo-dist's manifest and archives:
// @noref[bin/anchr.js, scripts/src/npm/build-packages.mjs]
//
//   @context-anchors/<os>-<cpu>   one per platform, holding just the binary
//   context-anchors               the shim: `bin/anchr.js` plus optionalDependencies on each
//                                 platform package, so npm installs exactly one of them
//
// This is the esbuild/biome layout. No postinstall download: it works under --ignore-scripts,
// offline, and with lockfile integrity, which cargo-dist's own npm installer does not.
//
//   node scripts/src/npm/build-packages.mjs --manifest dist-manifest.json \
//        --artifacts target/distrib --out npm-dist [--only-available] [--pack]
//
// --pack also packs each package to <out>/tarballs and writes the packages.json index that verify
// and publish consume; see @noref[scripts/src/npm/packages-index.mjs].

import { execFileSync } from "node:child_process";
import {
  chmodSync,
  copyFileSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { repoRoot } from "../repo-root.mjs";
import { INDEX_FILE, digestOf, npmInvocation } from "./packages-index.mjs";
import { parseArgs } from "node:util";

const SCOPE = "@context-anchors";
const SHIM_NAME = "context-anchors";
const BIN = "anchr";
const REPOSITORY = "https://github.com/Arkorri/context-anchors";
const LICENSE = "MIT OR Apache-2.0";
const LICENSE_FILES = ["LICENSE-MIT", "LICENSE-APACHE"];
const REPO_ROOT = repoRoot(import.meta.url);

// npm normalises a manifest only when publishing a *directory*; we publish tarballs, so anything it
// used to correct on the way in is ours to emit. At 0.0.1 that silently added the structured
// repository, the bugs link and the homepage.
function manifestMetadata() {
  return {
    repository: { type: "git", url: `git+${REPOSITORY}.git` },
    bugs: { url: `${REPOSITORY}/issues` },
    homepage: `${REPOSITORY}#readme`,
    license: LICENSE,
  };
}

function copyLicenses(packageDir) {
  for (const file of LICENSE_FILES) {
    copyFileSync(join(REPO_ROOT, file), join(packageDir, file));
  }
}

// Linux uses the static musl builds so one package per CPU covers glibc and musl systems.
const PLATFORMS = {
  "aarch64-apple-darwin": { os: "darwin", cpu: "arm64" },
  "x86_64-apple-darwin": { os: "darwin", cpu: "x64" },
  "aarch64-unknown-linux-musl": { os: "linux", cpu: "arm64" },
  "x86_64-unknown-linux-musl": { os: "linux", cpu: "x64" },
  "x86_64-pc-windows-msvc": { os: "win32", cpu: "x64" },
};

/// The archive in the manifest that carries the binary for one target triple.
function findArchiveName(manifest, triple) {
  return (
    Object.entries(manifest.artifacts ?? {}).find(
      ([, artifact]) =>
        artifact.kind === "executable-zip" && artifact.target_triples?.includes(triple),
    )?.[0] ?? null
  );
}

function versionOf(manifest) {
  const release = manifest.releases?.find((r) => r.app_name === SHIM_NAME);
  if (!release) throw new Error(`manifest has no release for ${SHIM_NAME}`);
  return release.app_version;
}

function platformPackageJson(version, platform) {
  return {
    name: `${SCOPE}/${platform.os}-${platform.cpu}`,
    version,
    description: `${BIN} binary for ${platform.os}-${platform.cpu}; installed by the ${SHIM_NAME} package`,
    ...manifestMetadata(),
    os: [platform.os],
    cpu: [platform.cpu],
    files: ["bin", ...LICENSE_FILES],
  };
}

/// One optional dependency per platform package actually built, all pinned to this version: npm
/// installs whichever matches and skips the rest.
function shimPackageJson(version, built) {
  return {
    name: SHIM_NAME,
    version,
    description:
      "anchr: a compiler-style checker for @anchor/@ref markers in docs, agent context files, and code comments",
    ...manifestMetadata(),
    bin: { [BIN]: `bin/${BIN}.js` },
    files: ["bin", "README.md", ...LICENSE_FILES],
    engines: { node: ">=18" },
    optionalDependencies: Object.fromEntries(built.map((name) => [name, version])),
  };
}

function main() {
  const { values: args } = parseArgs({
    options: {
      manifest: { type: "string" },
      artifacts: { type: "string" },
      out: { type: "string" },
      "only-available": { type: "boolean", default: false },
      pack: { type: "boolean", default: false },
    },
  });
  for (const required of ["manifest", "artifacts", "out"]) {
    if (!args[required]) {
      console.error(`missing --${required}`);
      process.exit(2);
    }
  }

  const manifest = JSON.parse(readFileSync(args.manifest, "utf8"));
  let version;
  try {
    version = versionOf(manifest);
  } catch (error) {
    console.error(error.message);
    process.exit(2);
  }

  rmSync(args.out, { recursive: true, force: true });
  mkdirSync(args.out, { recursive: true });

  const built = [];
  const packages = [];
  for (const [triple, platform] of Object.entries(PLATFORMS)) {
    const archiveName = findArchiveName(manifest, triple);
    const archivePath = archiveName ? join(args.artifacts, archiveName) : null;
    if (!archivePath || !existsSync(archivePath)) {
      if (args["only-available"]) {
        console.log(`skip ${triple}: no archive available`);
        continue;
      }
      console.error(
        archivePath
          ? `archive missing: ${archivePath}`
          : `manifest has no executable archive for ${triple}`,
      );
      process.exit(2);
    }

    const binaryName = platform.os === "win32" ? `${BIN}.exe` : BIN;
    const binary = extractBinary(archivePath, binaryName);
    const dir = join(args.out, SCOPE, `${platform.os}-${platform.cpu}`);
    mkdirSync(join(dir, "bin"), { recursive: true });
    copyFileSync(binary, join(dir, "bin", binaryName));
    chmodSync(join(dir, "bin", binaryName), 0o755);
    copyLicenses(dir);
    const packageJson = platformPackageJson(version, platform);
    writeJson(join(dir, "package.json"), packageJson);
    built.push(packageJson.name);
    packages.push({ name: packageJson.name, platform: `${platform.os}-${platform.cpu}`, dir });
    console.log(`built ${packageJson.name}`);
  }

  const shimDir = join(args.out, SHIM_NAME);
  mkdirSync(join(shimDir, "bin"), { recursive: true });
  writeFileSync(join(shimDir, "bin", `${BIN}.js`), shimSource(), { mode: 0o755 });
  writeFileSync(join(shimDir, "README.md"), readme());
  copyLicenses(shimDir);
  writeJson(join(shimDir, "package.json"), shimPackageJson(version, built));
  packages.push({ name: SHIM_NAME, shim: true, dir: shimDir });
  console.log(`built ${SHIM_NAME} with ${built.length} platform packages`);

  if (args.pack) {
    const destination = resolve(args.out, "tarballs");
    mkdirSync(destination, { recursive: true });
    const entries = packages.map(({ name, platform, shim, dir }) => {
      const filename = packPackage(resolve(dir), destination);
      const sha256 = digestOf(readFileSync(join(destination, filename)));
      return shim
        ? { name, shim: true, tarball: `tarballs/${filename}`, sha256 }
        : { name, platform, tarball: `tarballs/${filename}`, sha256 };
    });
    writeJson(join(args.out, INDEX_FILE), { version, packages: entries });
    console.log(`packed ${entries.length} tarballs and wrote ${INDEX_FILE}`);
  }
}

// Only the CLI invocation runs; importing the module for tests must not touch the filesystem.
if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main();
}

export {
  PLATFORMS,
  findArchiveName,
  findFile,
  manifestMetadata,
  parsePackOutput,
  platformPackageJson,
  readme,
  shimPackageJson,
  shimSource,
  versionOf,
};

/// `npm pack foo/bar` resolves as a GitHub shorthand rather than a folder, so pass no spec at all
/// and let `cwd` select the package. The filename comes from npm rather than being reconstructed —
/// guessing npm's naming rules is the same bet that lost the shim publish at 0.0.1.
function packPackage(packageDir, destination) {
  const { file, args } = npmInvocation(process.platform, [
    "pack",
    "--json",
    "--ignore-scripts",
    "--pack-destination",
    destination,
  ]);
  const output = execFileSync(file, args, { cwd: packageDir, encoding: "utf8" });
  const filename = parsePackOutput(output)?.[0]?.filename;
  if (!filename) throw new Error(`npm pack reported no filename for ${packageDir}`);
  return filename;
}

/// npm has been known to precede its JSON with notices; keep the raw text in the error so a
/// surprise is readable rather than a bare SyntaxError.
function parsePackOutput(output) {
  const start = output.indexOf("[");
  const end = output.lastIndexOf("]");
  if (start === -1 || end === -1) throw new Error(`npm pack --json produced no JSON:\n${output}`);
  return JSON.parse(output.slice(start, end + 1));
}

function extractBinary(archivePath, binaryName) {
  const scratch = mkdtempSync(join(tmpdir(), "anchr-npm-"));
  if (archivePath.endsWith(".zip")) {
    execFileSync("unzip", ["-q", "-o", archivePath, "-d", scratch], { stdio: "inherit" });
  } else {
    execFileSync("tar", ["-xf", archivePath, "-C", scratch], { stdio: "inherit" });
  }
  const found = findFile(scratch, binaryName);
  if (!found) {
    console.error(`${binaryName} not found inside ${archivePath}`);
    process.exit(2);
  }
  return found;
}

function findFile(dir, name) {
  for (const entry of readdirSync(dir)) {
    const path = join(dir, entry);
    if (statSync(path).isDirectory()) {
      const nested = findFile(path, name);
      if (nested) return nested;
    } else if (entry === name) {
      return path;
    }
  }
  return null;
}

function writeJson(path, value) {
  writeFileSync(path, `${JSON.stringify(value, null, 2)}\n`);
}

function shimSource() {
  return `#!/usr/bin/env node
"use strict";
const { spawnSync } = require("node:child_process");

const platformPackage = \`${SCOPE}/\${process.platform}-\${process.arch}\`;
const binaryName = process.platform === "win32" ? "${BIN}.exe" : "${BIN}";

let binary;
try {
  binary = require.resolve(\`\${platformPackage}/bin/\${binaryName}\`);
} catch {
  console.error(
    \`${BIN}: no prebuilt binary for \${process.platform}-\${process.arch} \` +
      \`(\${platformPackage} is not installed). Use the shell installer instead: ${REPOSITORY}\`,
  );
  process.exit(2);
}

const result = spawnSync(binary, process.argv.slice(2), { stdio: "inherit" });
if (result.error) {
  console.error(\`${BIN}: could not start \${binary}: \${result.error.message}\`);
  process.exit(2);
}
process.exit(result.status === null ? 1 : result.status);
`;
}

function readme() {
  return `# context-anchors

\`anchr\` checks \`@anchor[...]\` / \`@ref[...]\` markers in docs, agent context files, and code
comments the way a compiler checks identifiers: a reference that no longer resolves fails
\`anchr check\` and every site still using the old name is listed.

\`\`\`sh
npx context-anchors check
\`\`\`

This package installs a prebuilt native binary through a platform-specific optional dependency
(no postinstall download). Documentation: ${REPOSITORY}
`;
}
