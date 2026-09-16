---
title: Distribution
description: What ships, under which names, through which channels, and why the npm package is five platform packages plus a shim. Read before touching packaging, installers, or the binary's contents.
tags: [distribution, npm, cargo-dist, packaging]
---

# Distribution

<!-- refs -->
@ref[docs/RELEASING.md as Releasing]
@ref[#cli/init as Init]
@noref[bin/anchr.js]

How releases are cut, verified, and published is in @[Releasing]; this document is what a
release contains and where it goes.

## 1. Names
<!-- @anchor[dist/name] -->

| Role | Name |
|---|---|
| Project, repository, npm package, crate | `context-anchors` |
| Installed command | `anchr` |
| Markers | `@anchor`, `@ref`, `@noref` |

Package name and command name are independent: `context-anchors` is descriptive and searchable,
`anchr` is short to type, and `package.json`'s `bin` map plus Cargo's `[[bin]] name` let both
exist at once. `npx context-anchors check` resolves to the single bin; installed users type
`anchr`. The obvious short form `anchor` is the Solana framework's CLI, and anyone with both
installed would get a `PATH` collision. `anchr` is not reserved on any registry: nobody types the
command name into a registry, and a placeholder is clutter.

## 2. Artifacts

| Artifact | Channel |
|---|---|
| The binary: CLI and language server in one executable | GitHub Releases, npm |
| Agent integration: config, the marker guide, an optional hook | @[Init] |
| Editor extension | not planned; see §4 |

Only the binary is a packaging problem. @[Init] writes @ref[crates/context-anchors/templates/anchr.toml]
and the marker guide @ref[crates/context-anchors/templates/ANCHR.md] into the user's repository,
prints the one line to add to their agent instructions file, and with `--agent claude` also merges
a `PostToolUse` hook into the Claude Code settings file (read-modify-write, foreign keys
preserved). The vendor-neutral integration point is git: a pre-commit hook or a CI step running
`anchr check` catches every broken reference regardless of what did the editing, so per-vendor
hooks are built on demand, Claude Code first because it is the dogfooding environment.

## 3. Release tooling
<!-- @anchor[dist/cargo-dist] -->

cargo-dist (`dist`), configured in @ref[dist-workspace.toml], generates
@ref[.github/workflows/release.yml], cross-compiled archives for seven targets (macOS arm64 and
x64; Linux arm64 and x64, each glibc and static musl; Windows x64), shell and PowerShell
installers, checksums, and `cargo-binstall` metadata. Any change to the config needs
`dist generate` to refresh the workflow, or `dist plan` fails its stale-CI check. The npm packages
are not cargo-dist's; see §4.

## 4. Channels
<!-- @anchor[dist/channels] -->

**GitHub Releases and the installers.** The baseline: every platform, any CI, no registry
account. The one-line shell and PowerShell installers are in the README; the archives and
checksums are attached to each release.

**npm.** One tiny `context-anchors` package declaring `optionalDependencies` on five
`@context-anchors/<os>-<cpu>` packages, each holding only its binary. npm resolves exactly one by
`os` and `cpu`, and the shim `bin/anchr.js` execs it. Node is needed to *resolve* the package,
never to run the tool. Built from cargo-dist's manifest by
@ref[scripts/src/npm/build-packages.mjs], verified by @ref[scripts/src/npm/verify-packages.mjs],
published by @ref[scripts/src/npm/publish-packages.mjs]. This is the esbuild, swc, biome, and ruff
layout, and it is **not a postinstall download**: postinstall breaks under `--ignore-scripts`, in
air-gapped CI, behind corporate proxies, and defeats lockfile integrity. What npm buys over the
installer: `npx context-anchors check` needs no install step in a JS repository, and a lockfile
pins the version so every developer and CI run uses the same checker, which matters for a tool
that gates commits.

One known failure: npm's long-standing bug (npm/cli#4828, recurring as #8320) where regenerating
a lockfile on top of an existing `node_modules` records only the current machine's platform
package, so a teammate on another platform installs none. `npm ci` against a lockfile built from
scratch is unaffected. The shim's failure message names the missing package and points at the
shell installer, so this surfaces as an instruction, not a stack trace.

**Homebrew.** `brew install Arkorri/tap/context-anchors`. cargo-dist builds the formula from the
macOS and Linux archives and the `publish-homebrew-formula` job pushes it to the
`Arkorri/homebrew-tap` repository, a README plus one generated file. The tap is a separate
repository because Homebrew's `owner/name/formula` shorthand resolves to a repository called
`homebrew-name` under that owner, and because the publish job commits to the tap's default branch
on every release, which must not be this repository's `main`. It is also the cheap answer to
macOS trust:
`curl | sh` sets no quarantine attribute, so Gatekeeper never looks, but a browser-downloaded
archive gets one and is refused. Notarisation does not apply to Homebrew CLI formulae, so the tap
closes that path for free where a Developer ID costs money every year. Prereleases reach the tap
too, since Homebrew has no `next` tag; it orders `0.1.0` above `0.1.0-rc.1`, so the stable tag
restores the formula and `brew upgrade` moves anyone who installed during the rehearsal.

**Not planned.** The reasoning is kept here so the question is not reopened from scratch.

- crates.io: not a discovery channel for CLIs. `cargo binstall` is a real fetch path for Rust
  developers and the metadata already exists, but publishing means two crates versioned in
  lockstep for users who already have the shell installer.
- VS Code and OpenVSX: only VS Code needs an extension to reach a generic LSP server; Neovim,
  Helix, and Zed already work from `anchr lsp`.
- Code signing: unsigned survives while the installers and the tap are the recommended paths. On
  Windows, SmartScreen warns on browser-downloaded files that are double-clicked, not on a binary
  the PowerShell installer fetched and a terminal runs, and Microsoft's own guidance is that a
  signature does not stop the prompt until the file hash has download history, which every
  release resets. Azure Trusted Signing would add a paid subscription, an identity validation that
  puts the maintainer's legal name on every binary, and a renewal cycle, for a warning most users
  never see.

## 5. What is in the binary
<!-- @anchor[dist/implementation-decisions] -->

- **The LSP is a subcommand**, `anchr lsp` over stdio, not a second binary. One artifact, half the
  distribution surface; rust-analyzer, biome, and ruff do the same.
- **Grammars are the size problem.** Each bundled tree-sitter grammar is compiled C. The bundle is
  TypeScript and TSX, JavaScript, Python, Rust, and Go, roughly 5 MB together. A reference into a
  language without a grammar is *unverified*, and the diagnostic's hint names the fix, so
  "no grammar" is a packaging decision rather than a permanent limitation.

## 6. License

MIT or Apache-2.0, at the user's option: @ref[LICENSE-MIT] and @ref[LICENSE-APACHE]. For a tool
whose value depends on ubiquity, permissive is the only sensible choice.

## Rejected alternatives

- Our own platform packages over cargo-dist's npm installer: cargo-dist's is a single package that
  downloads the archive from GitHub at install time, which is exactly the postinstall pattern §4
  rules out.
- `anchorlint` as the name: "lint" contradicts the check-versus-lint distinction in
  @ref[docs/DESIGN.md] §7, and a name should not undercut the design.
- A short dictionary word as the name: roughly 45 candidates were checked and nearly every one is
  squatted on npm, which is why the viable space is compounds.
- Reserving `anchr` on the registries: clutter with no user.
- Publishing to crates.io: deferred on the cost of lockstep versioning, not on the discovery
  argument.
