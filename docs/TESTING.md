---
title: Testing
description: The one-test-file-per-module rule, what each layer of tests covers, the fuzz and spike targets, the dogfood check, and the CI job table with the local command for each. Read before adding a module, a test, or a CI job.
tags: [tests, ci, fuzz, coverage]
---

# Testing

<!-- refs -->
@ref[.github/workflows/ci.yml as CiWorkflow]
@noref[foo.rs, foo_tests.rs, _test.rs, lib.rs, main.rs, tests/]

## 1. The pairing rule

Every source file has exactly one sibling test file, named `foo_tests.rs` for `foo.rs`, declared
at the bottom of the source file, and never empty. This holds for all 48 source files today,
including `lib.rs` and `main.rs`. A new module ships with its test file in the same commit.

The plural is load-bearing: `cargo-llvm-cov` excludes `*_tests.rs` and a `tests/` directory from
its report, so coverage measures production code only. A singular `_test.rs` would silently count
test code as covered production lines.

The test file starts with `use super::*;`, so it sees private items. Fixtures are built in
`tempfile` directories by a small local helper (`Fixture`, `World`) inside the test file; there
is no shared test-utilities crate. Test names are sentences
(`root_selection_separates_typos_from_absence_for_every_kind`), and a test that pins a
non-obvious invariant carries a `///` comment saying which one.

@ref[crates/anchr-core/src/lib_tests.rs] is the odd one: it names one type from every module so
the crate root's module list is asserted, and its doc comment records which modules the binary
does not otherwise consume.

## 2. What each layer tests

- **Grammar and lexing** (@ref[crates/anchr-core/src/marker/]): table tests over every rejection
  reason in `parse_target` (qualified symbol, reserved characters, `.` and `..`, trailing slash,
  root prefix on each kind); the id, alias, symbol, and noref charsets; a `proptest` that any
  generated marker embedded in random text is found with the right span and that text without
  an opener yields none; a regression corpus for CRLF, `\r` in a body, multi-byte text before a
  marker, and a non-ASCII preceding character. Path containment is a second `proptest`: a parsed
  or anchored `RelPath` joined onto a root never escapes it.
- **Text regions** (@ref[crates/anchr-core/src/text/]): markdown fixtures for fenced and indented
  code, inline code, HTML comments, reference-style links, a marker split by `[`, an escaped
  marker, a fence inside a blockquote and a list item, tilde and longer closing fences, an
  unclosed fence at EOF. The complement approach's only failure mode is an imprecise code range
  letting an example through, so these are the tests that matter. Per language: comment
  extraction and a declaration table checked against a per-kind list written before the query
  (function, struct or class, interface, enum, type alias, method, exported const, module), plus
  one fixture with broken syntax around a declaration asserting *unverified* rather than
  *missing*. `LanguageRegistry::new()` succeeding is itself a test, so grammar or query drift
  fails CI.
- **Scan, index, resolution, check**: gitignored file matching `include` is not scanned; hidden
  directories are; `.git` is pruned; oversized and non-UTF-8 files are unverified with their
  anchors unindexed; undeclared root is an error while absent root is unverified; case-mismatched
  path misses on every platform; external root references are dropped; alias binding, undeclared
  and duplicate aliases; grouping by cause with sites sorted.
- **Commands and renderers** (binary crate): each command's pure decisions are unit-tested
  behind small seams; the renderers are tested on constructed reports.

## 3. Integration tests

@ref[crates/context-anchors/tests/] drives the real binary with `assert_cmd` and `predicates` on
tempdir fixtures: exit codes 0, 1, 2; `--format json` schema stability; `--strict`; `PATHS`
filtering; bad-config caret rendering; `init` idempotency, `--force`, `--dry-run`, and settings
merging that preserves foreign keys; `backrefs`, `rename`, `coverage`, `annotate`.
@ref[crates/context-anchors/tests/lsp.rs] is a hand-rolled JSON-RPC client over the binary's
stdio with a read timeout, so a protocol mistake fails instead of hanging.

The three `insta` snapshots beside @ref[crates/context-anchors/tests/cli.rs] pin the human report
(grouped by cause, with `--color never`) and the JSON report. The snapshot directory is in
`[ignore] paths`, so it is neither scanned nor referenceable. After an intentional output change:

```sh
cargo insta review
```

## 4. Spike tests

@ref[crates/anchr-core/tests/walker_behaviour.rs] pins third-party behaviour the scan depends
on: `require_git`, override precedence over ignore files, symlink handling, and that a panic on a
walker thread propagates. If one fails after a dependency bump, the assumption changed and the
scan stage needs a look, not the test.

## 5. Fuzzing

@ref[fuzz/] is its own workspace with five libFuzzer targets in @ref[fuzz/fuzz_targets/]:
`parse-target`, `lex`, `markdown-regions`, `source-regions`, `config`. Each asserts that the
function never panics or hangs and that every returned span lies inside the input. The markdown
target restores the default panic hook so the narrow `catch_unwind` around pulldown-cmark is
exercised too. CI smoke-runs each for 60 seconds on nightly; locally:

```sh
cargo +nightly fuzz run lex -- -max_total_time=60
```

## 6. Dogfood

@ref[anchr.toml] at the root makes this repository its own fixture, and CI fails on any broken
reference:

```sh
cargo run --locked --bin anchr -- check --strict --color never
```

Conventions for markers in this repository's documents and comments: a type or module mentioned
more than once is declared once in the file's `<!-- refs -->` block and written as `@[Alias]` at
every mention; a one-off mention is a path or symbol reference; cross-document citations are
anchor references; same-document `§` citations stay numeric. Example markers live in fences or
inline code, which is itself a test of exclusion. Run `anchr coverage` after editing prose: a new
"could be" row is a mention that should be a marker, and an "ignored but never matched" row is a
`@noref` entry to delete.

## 7. Scripts

@ref[scripts/src/] is mirrored one-to-one by @ref[scripts/tests/], run with `node --test`. Every
script guards its `main()` behind an argv check so a test can import its pure functions without
fetching or writing.

```sh
node --test 'scripts/tests/**/*.test.mjs'
```

## 8. Coverage floors

CI fails under 88% workspace lines or 70% on any single file. Both ratchet upward as gaps close.
The pairing rule (§1) guarantees a test file exists; the floors guarantee it tests something,
which is why a per-file assertion mandate is not needed.

```sh
cargo llvm-cov --workspace --locked --no-report
cargo llvm-cov report --summary-only
cargo llvm-cov report --fail-under-file-lines 70
cargo llvm-cov report --fail-under-lines 88
```

## 9. CI jobs

Every job in @[CiWorkflow] is a command you can run locally.

| Job | What it checks | Local command |
|---|---|---|
| `check` (ubuntu, macos, windows) | format, clippy with warnings denied, all tests | `cargo fmt --all -- --check`; `cargo clippy --workspace --all-targets --all-features --locked`; `cargo test --workspace --locked --no-fail-fast` |
| `coverage` | the floors in §8 | §8 |
| `dogfood` | the repository's own references | §6 |
| `linguist-table` | the generated extension table is fresh | `node scripts/src/linguist/gen-extensions.mjs --check` |
| `docs-index` | the generated docs index is fresh | `node scripts/src/docs/gen-index.mjs --check` |
| `scripts` | script unit tests | §7 |
| `msrv` | the workspace builds on Rust 1.85 | `cargo +1.85 build --workspace --locked` |
| `supply-chain` | advisories, licenses, bans, sources | `cargo deny check` |
| `fuzz` (five targets) | 60 seconds per target on nightly | §5 |

Warnings are errors in CI (`RUSTFLAGS: -D warnings`); run clippy the same way locally.

## Rejected alternatives

- Coverage floors over a per-file assertion mandate: a mandate is satisfied by a test that asserts
  nothing; a floor is not.
- A shared test-utilities crate over local `Fixture` helpers: each file's fixture is a few lines
  and differs in what it needs; a shared one would grow options for every caller.
- Checked-in fixture repositories over tempdir fixtures built inline: the fixture is readable
  next to the assertion, and nothing on disk drifts.
- Snapshot tests for every report over three: the grouped human report, the relative-path
  malformed case, and the JSON schema are the shapes that must not drift; everything else is
  asserted directly.
