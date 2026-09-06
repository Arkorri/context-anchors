#!/usr/bin/env node
// Builds the npm packages for one release from cargo-dist's manifest and archives:
//
//   @context-anchors/<os>-<cpu>   one per platform, holding just the binary
//   context-anchors               the shim: `bin/anchr.js` plus optionalDependencies on each
//                                 platform package, so npm installs exactly one of them
//
// This is the esbuild/biome layout. No postinstall download: it works under --ignore-scripts,
// offline, and with lockfile integrity, which cargo-dist's own npm installer does not.
//
//   node scripts/npm/build-packages.mjs --manifest dist-manifest.json \
//        --artifacts target/distrib --out npm-dist [--only-available]

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
import { join } from "node:path";
import { parseArgs } from "node:util";

const SCOPE = "@context-anchors";
const SHIM_NAME = "context-anchors";
const BIN = "anchr";
const REPOSITORY = "https://github.com/averykempton/context-anchors";
const LICENSE = "MIT OR Apache-2.0";

// Linux uses the static musl builds so one package per CPU covers glibc and musl systems.
const PLATFORMS = {
  "aarch64-apple-darwin": { os: "darwin", cpu: "arm64" },
  "x86_64-apple-darwin": { os: "darwin", cpu: "x64" },
  "aarch64-unknown-linux-musl": { os: "linux", cpu: "arm64" },
  "x86_64-unknown-linux-musl": { os: "linux", cpu: "x64" },
  "x86_64-pc-windows-msvc": { os: "win32", cpu: "x64" },
};

const { values: args } = parseArgs({
  options: {
    manifest: { type: "string" },
    artifacts: { type: "string" },
    out: { type: "string" },
    "only-available": { type: "boolean", default: false },
  },
});
for (const required of ["manifest", "artifacts", "out"]) {
  if (!args[required]) {
    console.error(`missing --${required}`);
    process.exit(2);
  }
}

const manifest = JSON.parse(readFileSync(args.manifest, "utf8"));
const release = manifest.releases?.find((r) => r.app_name === SHIM_NAME);
if (!release) {
  console.error(`manifest has no release for ${SHIM_NAME}`);
  process.exit(2);
}
const version = release.app_version;

rmSync(args.out, { recursive: true, force: true });
mkdirSync(args.out, { recursive: true });

const built = [];
for (const [triple, platform] of Object.entries(PLATFORMS)) {
  const archiveName = Object.entries(manifest.artifacts).find(
    ([, artifact]) =>
      artifact.kind === "executable-zip" && artifact.target_triples?.includes(triple),
  )?.[0];
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
  const packageName = `${SCOPE}/${platform.os}-${platform.cpu}`;
  const dir = join(args.out, SCOPE, `${platform.os}-${platform.cpu}`);
  mkdirSync(join(dir, "bin"), { recursive: true });
  copyFileSync(binary, join(dir, "bin", binaryName));
  chmodSync(join(dir, "bin", binaryName), 0o755);
  writeJson(join(dir, "package.json"), {
    name: packageName,
    version,
    description: `${BIN} binary for ${platform.os}-${platform.cpu}; installed by the ${SHIM_NAME} package`,
    repository: REPOSITORY,
    license: LICENSE,
    os: [platform.os],
    cpu: [platform.cpu],
    files: ["bin"],
  });
  built.push(packageName);
  console.log(`built ${packageName}`);
}

const shimDir = join(args.out, SHIM_NAME);
mkdirSync(join(shimDir, "bin"), { recursive: true });
writeFileSync(join(shimDir, "bin", `${BIN}.js`), shimSource(), { mode: 0o755 });
writeFileSync(join(shimDir, "README.md"), readme());
writeJson(join(shimDir, "package.json"), {
  name: SHIM_NAME,
  version,
  description: "anchr: a compiler-style checker for @anchor/@ref markers in docs, agent context files, and code comments",
  repository: REPOSITORY,
  license: LICENSE,
  bin: { [BIN]: `bin/${BIN}.js` },
  files: ["bin", "README.md"],
  engines: { node: ">=18" },
  optionalDependencies: Object.fromEntries(built.map((name) => [name, version])),
});
console.log(`built ${SHIM_NAME} with ${built.length} platform packages`);

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
