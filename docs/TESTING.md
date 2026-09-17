---
title: Testing
description: The one-test-file-per-module rule, what each layer of tests covers, the fuzz and spike targets, the dogfood check, the CI job table with the local command for each, and the required checks and rulesets on main. Read before adding a module, a test, or a CI job.
tags: [tests, ci, fuzz, coverage]
---

# Testing

<!-- refs -->
@ref[.github/workflows/ci.yml as CiWorkflow]
@ref[docs/RELEASING.md as Releasing]
@noref[foo.rs, foo_tests.rs, _test.rs, lib.rs, main.rs, tests/]

## 1. The pairing rule

Every source file has exactly one sibling test file, named `foo_tests.rs` for `foo.rs`, declared
at the bottom of the source file, and never empty. This holds for all 48 source files today,
including `lib.rs` and `main.rs`. A new module ships with its test file in the same commit.

The plural is load-bearing: `cargo-llvm-cov` excludes `*_tests.rs` and a `tests/` directory from
its report, so coverage measures production code only. A singular `_test.rs` would silently count
test code as covered production lines.

The test file starts with `use super::*;`, so it sees private items. Unit-test fixtures are built
by a small local helper (`World`) inside the test file; the integration binary shares one
`Fixture` (§3). There is no shared test-utilities crate. Test names are sentences
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

@ref[crates/context-anchors/tests/integration/] is one test binary that drives the real `anchr`
with `assert_cmd` and `predicates` on tempdir fixtures. One binary rather than one crate per file,
so the modules share a fixture and the dev-dependencies link once; tests share a process, so
nothing may touch global state. A module per command: `cli` (exit codes 0, 1, 2; `--format json`
schema stability; `--format github` annotations and their workspace-relative paths; `--strict`;
`PATHS` filtering; bad-config caret rendering), `init` (idempotency,
`--force`, `--dry-run`, settings merging that preserves foreign keys), `tools` (`backrefs`,
`rename`), `coverage` (`coverage`, `annotate`), and `lsp`, a hand-rolled JSON-RPC client over the
binary's stdio with a read timeout, so a protocol mistake fails instead of hanging. Filter by
module: `cargo test --test integration cli::`.

@ref[crates/context-anchors/tests/integration/support.rs] holds the one `Fixture`. Its
constructor takes the files and nothing else: every fixture gets a `.git` marker, and every
command runs with `NO_COLOR` removed and the home and git-config variables pointed at an empty
directory inside the fixture, so the developer's global gitignore never reaches a test. A test
that needs an unusual layout builds it through `root()` inside the test; a helper is promoted
into the support module only when a second module wants it; the constructor never grows a
parameter.

The four `insta` snapshots beside @ref[crates/context-anchors/tests/integration/cli.rs] pin the human report
(grouped by cause, with `--color never`), the JSON report, and the GitHub annotation list. The
snapshot directory is in
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
reference. It is the last step of the `rust` job, so it runs on all three platforms with the
binary the tests just built, and `--format github` puts each finding on the pull-request diff.
Locally:

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

@[CiWorkflow] is the one workflow a pull request runs; the release workflow runs only on tags
(@[Releasing] §2). It runs on every pull request and on every push to `main`. A new push to a
pull request cancels its running checks; a run on `main` is never cancelled, because it is the
only test of the merged tree (§10). The workflow token has `contents: read` and every job has a
timeout. Every job is a command you can run locally.

| Job | What it checks | Local command |
|---|---|---|
| `rust` (ubuntu, macos, windows) | format, clippy with warnings denied, all tests, then the repository's own references (§6) | `cargo fmt --all -- --check`; `cargo clippy --workspace --all-targets --all-features --locked`; `cargo test --workspace --locked --no-fail-fast`; `cargo run --locked --bin anchr -- check --strict` |
| `coverage` | the floors in §8; a separate instrumented build, so its test run is the measurement and not a repeat | §8 |
| `msrv` | the workspace and its tests type-check on Rust 1.85 | `cargo +1.85 check --workspace --all-targets --locked` |
| `generated` | script unit tests (§7); the generated extension table and docs index are fresh; the user guide builds as a site (§11); the generated release workflow is fresh against @ref[dist-workspace.toml] | `node --test 'scripts/tests/**/*.test.mjs'`; `node scripts/src/linguist/gen-extensions.mjs --check`; `node scripts/src/docs/gen-index.mjs --check`; `node scripts/src/site/build.mjs && mdbook build site`; `dist plan` |
| `supply-chain` | advisories, licenses, bans, sources | `cargo deny check` |
| `action` (ubuntu, macos, windows) | the composite action at @ref[action.yml] installs the latest release and checks the repository with it, so it proves the install path on every platform and lags `rust` by one release: a PR that needs a flag newer than the last release turns it red until that release ships | none; `anchr check --strict --format github` with an installed binary is the equivalent |
| `fuzz` (five targets) | 60 seconds per target on nightly | §5 |
| `ci-ok` | every job above succeeded; the one required check (§10) | none |

Warnings are errors in CI (`RUSTFLAGS: -D warnings`); run clippy the same way locally.

## 10. Required checks
<!-- @anchor[tests/required-checks] -->

`main` accepts only squash merges of pull requests whose `ci-ok` check passed. The rule is a
repository ruleset, kept as @ref[.github/rulesets/main.json] and applied with the GitHub API,
so a change to it is reviewed like any other:

```sh
gh api -X POST repos/Arkorri/context-anchors/rulesets --input .github/rulesets/main.json
gh api repos/Arkorri/context-anchors/rulesets --jq '.[] | {id, name, enforcement}'
gh api -X PUT repos/Arkorri/context-anchors/rulesets/<id> --input .github/rulesets/main.json
```

What it requires and why:

- **One check, `ci-ok`**, reported by GitHub Actions (`integration_id` 15368) and nothing else.
  It needs every job in @[CiWorkflow], so a job added or renamed is covered without a settings
  change. The individual jobs still show on the pull request; they are just not what the merge
  waits for.
- **Not strict.** A branch need not be up to date with `main` for its checks to count. The
  repository merges stacked pull requests bottom-up, and strict checks would demand a rebase and
  a fresh run per layer. The run on `main` after each merge tests the merged tree instead.
- **Squash only**, with linear history, so `main` reads as one `type(scope): summary (#N)` line
  per pull request. No review count is required: this is a single-maintainer repository.
- **Administrators bypass.** The `action` job is red by design when a pull request needs a flag
  newer than the last release (§9); that is the case the bypass exists for. GitHub Actions' own
  token is not an administrator, so no workflow can push to `main`.

Release tags have their own ruleset; see @[Releasing] §1.

If concurrent contributors ever make "not strict" insufficient, the upgrade is a merge queue:
add `merge_group:` to the workflow's triggers and a `merge_queue` rule to the ruleset. Nothing
in the generated release workflow is involved, because the `dist plan` check already runs in
@[CiWorkflow].

## 11. The documentation site
<!-- @anchor[tests/site] -->

The user guide under @ref[docs/guide/] is published at https://arkorri.github.io/context-anchors/
by @ref[.github/workflows/pages.yml], which runs on a push to `main` that touches the guide, the
site config, or the scripts, and never on a pull request. The pages are written with this
repository's markers so that `anchr check` covers them (§6); @ref[scripts/src/site/build.mjs]
copies them under @ref[site/] with every marker rewritten as a link and writes the sidebar, and
mdBook renders the result with @ref[site/book.toml]. The rewrite is the site's test: an alias
with no declaration, an anchor declared outside the guide, or a reference into another root is
an error, so the `generated` job (§9) builds the site on every pull request and a dead link never
reaches `main`. Markers in fences and inline code are left as written, which is how the guide
shows the syntax. The mdBook version both workflows install is pinned in the script.

```sh
node scripts/src/site/build.mjs && mdbook build site     # or `mdbook serve site` to preview
```

## Rejected alternatives

- Coverage floors over a per-file assertion mandate: a mandate is satisfied by a test that asserts
  nothing; a floor is not.
- One integration crate per file over one binary: each crate links the dev-dependencies again
  and cannot share a module, so the fixture was pasted three times and the copies drifted (the
  `.git` marker and the `NO_COLOR` scrub were lost on the way).
- A workspace test-utilities crate over a support module in the integration binary: the core
  crate's helpers build in-memory worlds, not repositories on disk, so there is nothing to share
  yet; and a fixture with constructor options grows one for every caller, which is why the
  support module's constructor takes none.
- Checked-in fixture repositories over tempdir fixtures built inline: the fixture is readable
  next to the assertion, and nothing on disk drifts.
- Snapshot tests for every report over four: the grouped human report, the relative-path
  malformed case, the JSON schema, and the GitHub annotation list are the shapes that must not
  drift; everything else is asserted directly.
- Requiring each CI job by name over one fan-in: every rename or new job would edit the ruleset,
  and a matrix job's name carries the runner label, which changes too.
- Strict required checks over a post-merge run on `main`: strict serialises a stack into one
  rebase and one CI round per layer for a merged-tree guarantee the `main` run already gives.
- A merge queue today: it exists to interleave concurrent contributors, and there are none.
