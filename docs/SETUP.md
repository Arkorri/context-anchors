---
title: Setup
description: The toolchain, the tools to install, and the local commands that reproduce every CI check. Read once when cloning, and again when a CI job fails that you have never run locally.
tags: [setup, toolchain, ci]
---

# Setup

<!-- refs -->
@ref[docs/TESTING.md as Testing]
@noref[npm-dist/]

## Toolchain

- **Rust stable**, selected by @ref[rust-toolchain.toml] with `rustfmt`, `clippy`, and
  `llvm-tools-preview`. The workspace's minimum supported version is 1.85 and CI builds on it;
  nothing in the code should need newer.
- **Rust nightly** only for fuzzing: `rustup toolchain install nightly`.
- **Node 24 or later** for the scripts under @ref[scripts/]. No `package.json`, no dependencies.
- **A C compiler** at build time, for the tree-sitter grammar crates. Xcode command-line tools,
  `build-essential`, or MSVC.

Cargo tools, installed once:

```sh
cargo install cargo-llvm-cov cargo-insta cargo-deny --locked   # coverage, snapshots, supply chain
cargo install cargo-fuzz --locked                               # fuzzing (needs nightly)
cargo install cargo-dist --locked                               # only when touching releases
```

## Build and run

```sh
git clone https://github.com/Arkorri/context-anchors && cd context-anchors
cargo build --locked
cargo run --locked --bin anchr -- check --strict       # anchr checks this repository
cargo run --locked --bin anchr -- coverage             # what could still be marked
```

`--locked` everywhere: @ref[Cargo.lock] is the tested dependency set.

## Before you push

The sequence below is what CI runs (job by job in @[Testing] §9). Run the parts your change
touches; run all of it before opening a PR.

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked
cargo test --workspace --locked --no-fail-fast
cargo run --locked --bin anchr -- check --strict --color never
node --test 'scripts/tests/**/*.test.mjs'
node scripts/src/linguist/gen-extensions.mjs --check
node scripts/src/docs/gen-index.mjs --check
cargo deny check
dist plan
```

Set `RUSTFLAGS=-D warnings` to match CI exactly. Coverage floors and fuzz smoke runs are in
@[Testing] §8 and §5. `dist plan` only matters after editing @ref[dist-workspace.toml] and
needs cargo-dist at the version that file names.

## Working on the docs

Every file under @ref[docs/] carries frontmatter with `title`, `description`, and `tags`; the
index @ref[docs/README.md] is generated from it. After adding or renaming a document, or editing
its frontmatter:

```sh
node scripts/src/docs/gen-index.mjs
```

Anything that names a file, a declaration, or a section is an `@ref`, and `anchr check` fails
the build if it stops resolving. The marker syntax is in @ref[docs/DESIGN.md] §3; authoring
conventions for this repository are in @[Testing] §6.

## Language server in an editor

`anchr lsp` speaks stdio. Point any generic LSP client at the debug binary for a live loop:

```sh
cargo build --locked && echo target/debug/anchr
```

Configure the editor to run that path with the argument `lsp` for markdown files. Diagnostics,
go-to-definition, references, rename, and document symbols come from the same core as the CLI.

## Release scripts locally

The npm packaging scripts consume cargo-dist's output. To exercise them without a release:

```sh
dist build --artifacts=all --output-format=json > dist-manifest.json
node scripts/src/npm/build-packages.mjs --manifest dist-manifest.json --artifacts target/distrib --out npm-dist --only-available --pack
node scripts/src/npm/verify-packages.mjs --dir npm-dist --platform <os-cpu> --expect-version <version>
```

`--only-available` skips platforms the local machine did not build. `npm-dist/` is gitignored.
How releases actually run is in the releasing guide.

## Rejected alternatives

- Pinning a Rust version in @ref[rust-toolchain.toml] over `stable`: the MSRV job already catches
  a too-new feature, and a pin would make every contributor's toolchain drift from the one that
  ships.
- A `Makefile` or `justfile` over the command list above: eight commands that mirror the CI file
  one to one need no third place to be defined.
