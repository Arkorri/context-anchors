---
title: Code style
description: The conventions clippy cannot enforce, from module layout and validated newtypes to the two error channels and the rules for untrusted input. Read before writing or reviewing Rust in this repository.
tags: [rust, conventions, security]
---

# Code style

<!-- refs -->
@ref[Cargo.toml as WorkspaceManifest]
@ref[docs/ARCHITECTURE.md as Architecture]
@ref[docs/TESTING.md as Testing]
@noref[foo/foo.rs, foo_tests.rs, mod.rs, foo.rs, foo/, lib.rs, main.rs]

Mechanical rules live in `[workspace.lints]` in @[WorkspaceManifest] and in `rustfmt`, and CI runs
both with warnings denied. This document is only what a linter cannot say.

## 1. Layout

Every module is a directory holding the module file and its test file, both named for the
module: `foo/foo.rs` beside `foo_tests.rs`. There is no `mod.rs` anywhere, and no `foo.rs`
beside a `foo/` directory. The parent declares the child with a path attribute and the child
declares its own tests at the bottom of the file:

```rust
// marker/marker.rs
#[path = "lex/lex.rs"]
mod lex;

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod marker_tests;
```

A `#[path]` module's children resolve against the directory holding it, so the test module needs
no attribute. The crate roots are the one exception: Cargo fixes them at `lib.rs` and `main.rs`,
so they pair flat with a sibling test file. Parents re-export what callers need
(`pub use lex::...`) so consumers name `marker::Marker`, never the file.

Nesting reaches three levels (`marker/lex`, `coverage/linguist`). A module that needs a fourth is
two modules. Why the pairing is a rule, and why the plural `_tests` matters, is in @[Testing].

## 2. Types

- **Every identifier is a validated newtype** with a private field, `pub fn parse(&str) ->
  Result<Self, XError>`, `as_str()`, `Display`, and an explicit `MAX_*_BYTES` constant next to
  it: @ref[crates/anchr-core/src/marker/id/id.rs#AnchorId],
  @ref[crates/anchr-core/src/marker/alias/alias.rs#Alias],
  @ref[crates/anchr-core/src/marker/symbol/symbol.rs#SymbolName],
  @ref[crates/anchr-core/src/root/root.rs#RootName], @ref[crates/anchr-core/src/marker/path/path.rs#RelPath],
  @ref[crates/anchr-core/src/root/root.rs#FilePath]. Validation happens once at construction; the
  rest of the code trusts the type. No `FromStr`: parsing is a named, deliberate act.
- **Enums, never booleans, for a fixed choice** (`UnverifiedPolicy`, `PathExpectation`,
  `ScanMode`, `Severity`). Adding a variant must make every match fail to compile, which is also
  why core enums carry no `#[non_exhaustive]`.
- **Paths are `camino` UTF-8 paths end to end.** `std::path` appears only where the OS hands us
  one, and the conversion happens at that boundary. The two path types and why there are two are
  in @[Architecture] §5.
- **Spans are half-open byte offsets.** Line and column are computed at the edge (diagnostics,
  LSP), 1-based for display, and narrowed to `u32` only through `try_from`.
- Plain structs with `pub` fields are fine for data that has no invariant. The one builder is
  `ReportBuilder`, which exists because grouping and sorting must happen before the report is
  observable.

## 3. Errors

Two channels, kept apart on purpose.

- **Findings about the user's files are data**, never `Err`: broken references, malformed
  markers, a file too large or not UTF-8, a walk problem. They live in
  @ref[crates/anchr-core/src/diagnostic/diagnostic.rs#Report]. One bad file never aborts a run.
- **Tool failures are errors.** In `anchr-core` every fallible function returns a `thiserror`
  enum, roughly one per module or newtype (`ConfigError`, `LexError`, `PathError`, ...), and
  @ref[crates/anchr-core/src/check/check.rs#CheckError] aggregates them with `#[from]`. Errors
  that end up inside a diagnostic key derive `Clone, PartialEq, Eq, Hash`. The core never uses
  `anyhow`.
- **The binary uses `anyhow`** to add context and maps to exit codes in
  @ref[crates/context-anchors/src/main.rs]: 0 clean, 1 errors in the report, 2 the tool failed.
- **A panic is a bug, never control flow.** No `unwrap` or `expect` in production code (denied
  by lint). Exactly two `catch_unwind`s exist, each around code we do not own: the pulldown-cmark
  iterator, which panics on some malformed documents, and each LSP message, so one panic cannot
  kill an editor session. Neither wraps our own per-file work; swallowing our panics would hide
  the defects fuzzing exists to surface.

## 4. Lint escapes

- Production code that must call `expect` (a literal regex in a `LazyLock`) uses
  `#[expect(clippy::expect_used, reason = "...")]` on the item, so the escape is removed
  automatically when it stops being needed. See @ref[crates/anchr-core/src/marker/lex/lex.rs].
- Test modules get `#[allow(clippy::unwrap_used)]` (and `expect_used` where needed) on the `mod`
  declaration in the parent, not inside the test file. Integration test files carry the crate-level
  `#![allow(...)]` at the top.
- `#[allow(deprecated)]` is acceptable only for a field a third-party protocol type forces on us,
  with the reason in a comment.
- There are no other `#![...]` crate attributes; the workspace lints are the policy.

## 5. Comments

Comment only where a competent reader would otherwise be wrong. Names carry the weight.

- A module doc (`//!`) states the module's purpose and the one design rule that governs it, in
  one to three sentences.
- An item doc (`///`) justifies a decision or names a non-obvious constraint. It never restates
  the signature.
- Comments carry live markers: a comment that names a file, a declaration, or an anchor writes
  `@ref[...]`, and the repository checks its own comments in CI. Example strings that look like
  paths go in a `@noref[...]` comment near the top of the file.
- No `TODO` in code. Open work is tracked in GitHub issues, not in the repository.

## 6. Untrusted input
<!-- @anchor[code/security] -->

Every walked file, every config file, and every marker body is untrusted. A hostile repository
must not crash the checker, read outside its roots, or exhaust memory. The rules:

- **Bounds are explicit constants**, named `MAX_*`, beside the type they bound: id and symbol
  bytes, alias bytes, path bytes, noref entry bytes, declarations per file, symlink hops. File size
  is checked from metadata before reading (`max-file-bytes`). No `with_capacity(n)` where `n`
  derives from file content.
- **Allowlists, not denylists**, for every parsed charset. Reject; never "clean".
- **No `std::process::Command` anywhere.** The git root is found by walking up for `.git`.
- **Reads are contained.** The only reads outside the walk are symbol-resolution targets, and
  each is canonicalised and checked with `starts_with` against the canonical root first. The
  walker never follows links. Existence-only checks consult the scan tree, not the disk.
- **Offsets from a parser are untrusted.** Slice with `get(..)`, never `[..]`, at a byte offset a
  parser produced; narrow with `try_from`, never `as`; `overflow-checks` stays on in release.
- **Parsing has a budget.** Tree-sitter runs under a wall-clock progress callback; the marker and
  token regexes are `regex` (linear time); a declaration table that hits its cap is discarded,
  not truncated.
- **Config is parsed defensively but its roots are trusted.** `deny_unknown_fields`, enums for
  every fixed choice, collection caps validated right after deserialisation, `~` expanded only in
  `[roots]`. A config that points a root at `~` walks `~`; that is the user's choice.
- **Output is bounded.** JSON carries paths, spans, marker bodies, ids, and suggestions, never
  file content. The human renderer prints only lines that hold a marker site in a scanned
  container, and only for the first site of each diagnostic.
- Third-party walker and gitignore behaviour these rules depend on is pinned by spike tests
  (@[Testing]). If one fails after a dependency bump, the assumption changed.

## 7. Serde

`Deserialize` is derived only in @ref[crates/anchr-core/src/config/config.rs], on private raw
structs with `#[serde(deny_unknown_fields, rename_all = "kebab-case", default)]`, wrapped in
`toml::Spanned` where a semantic error needs a line. The raw structs are validated once into a
plain `Config` that carries no serde. `Serialize` is derived only in the binary's JSON renderer,
on DTOs that are decoupled from the core types and versioned by `schema`.

## 8. Dependencies

Reuse first: the only from-scratch pieces are the target grammar, the region complement, the
index, resolution, and grouping, which is where the novelty is. Before adding a crate: check the
name, downloads, repository, and maintainer; grammar crates compile C in `build.rs` and are vetted
individually. @ref[deny.toml] enforces the license allowlist and bans unknown registries and git
sources; @ref[Cargo.lock] is committed and CI builds `--locked`. Versions are declared once in
`[workspace.dependencies]`.

## 9. Generated code

@ref[crates/anchr-core/src/coverage/linguist/linguist.rs] is written by
@ref[scripts/src/linguist/gen-extensions.mjs] and says so in its header; CI regenerates and
fails on a diff. The same holds for @ref[docs/README.md] and @ref[scripts/src/docs/gen-index.mjs].
Never edit a generated file by hand; change the generator or its input and rerun.

## Rejected alternatives

- Inline `#[cfg(test)] mod tests` blocks over sibling files: a source file free of tests reads as
  the module, and coverage tooling can exclude the sibling by name.
- `missing_docs` as a lint: it would force a doc comment on every public item, against the rule
  that a comment exists only where the reader would otherwise be wrong.
- `#[non_exhaustive]` on core enums: it applies across crates, so the binary's renderers would
  need `_ =>` arms and lose "add a variant, every match fails to compile".
- `catch_unwind` around per-file work: it would hide our own defects; only third-party code and
  the editor session are isolated.
- `anyhow` in the core: callers could not match on the failure without string comparison.
