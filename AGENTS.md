# Working in this repository

`anchr` makes references in prose checkable: `@anchor[id]` names a line, `@ref[target]` asserts
a path, declaration, or anchor exists, and `anchr check` fails when one stops resolving. This
repository checks its own docs and comments in CI, so every rule below is also enforced on you.
<!-- @noref[foo.rs, foo_tests.rs, mod.rs] -->

## Layout

- @ref[crates/anchr-core/]: the library. Scan, lex, index, resolve, group. Everything testable.
- @ref[crates/context-anchors/]: the `anchr` binary. Argument parsing, rendering, exit codes, LSP.
- @ref[scripts/]: Node scripts for generated files and npm packaging, tests beside them.
- @ref[docs/]: developer docs. Start at @ref[docs/README.md]; it is generated from frontmatter.

## Commands

```sh
cargo build --locked
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked     # CI denies warnings
cargo test --workspace --locked --no-fail-fast
cargo run --locked --bin anchr -- check --strict --color never     # the repo's own references
cargo run --locked --bin anchr -- coverage                          # mentions that could be markers
node --test 'scripts/tests/**/*.test.mjs'
node scripts/src/docs/gen-index.mjs                                 # after editing docs frontmatter
```

## Rules you cannot derive from the code

- **Every source file has a sibling test file**, `foo_tests.rs` beside `foo.rs`, declared at the
  bottom of the source file, never empty. Modules are directories wired with `#[path]`; there is
  no `mod.rs`. A new module ships with its test file in the same commit. The rule covers `src/`;
  a support module inside the integration test binary is exercised by every test that imports it
  and has no sibling.
- **Findings are data, tool failures are errors.** A broken reference or a bad file goes into the
  report; `Err` means the tool could not run. The core never uses `anyhow`.
- **No `unwrap` or `expect` in production code.** When one is unavoidable, use
  `#[expect(clippy::expect_used, reason = "...")]`.
- **Every path, declaration, or anchor named in prose or a comment is an `@ref`.** A name used
  more than once in a file is declared once under `<!-- refs -->` as `@ref[target as Name]` and
  written `@[Name]` after that. Example markers go in code fences or inline code. Run
  `anchr check --strict` before committing anything that mentions one, and `anchr coverage` after
  editing prose.
- **Never edit a generated file by hand**: the Linguist table, @ref[docs/README.md], and
  @ref[.github/workflows/release.yml]. Change the generator or its input and rerun.
- **Every file under @ref[docs/] starts with frontmatter** (`title`, `description`, `tags`), then
  the index is regenerated.
- **Open work goes in @ref[TODO.md]**, one line each. No `TODO` comments in code.
- **Commits and PR titles** are `type(scope): summary` (`feat(coverage): ...`,
  `chore(docs): ...`). Branches are `<initials>/<MM_DD_YY>/<type>_<scope>_/<slug>`.

## Before you change something, read

| Change | Read first |
|---|---|
| A module, a type, a stage of the pipeline | @ref[docs/ARCHITECTURE.md] |
| What a marker means, what `check` reports, an invariant | @ref[docs/DESIGN.md] |
| Rust conventions, error handling, untrusted input | @ref[docs/CODE_STYLE.md] |
| A test, a fixture, a CI job | @ref[docs/TESTING.md] |
| Aliases or ignores | @ref[docs/design/aliases.md], @ref[docs/design/ignores.md] |
| Packaging, installers, the npm layout | @ref[docs/DISTRIBUTION.md] |
| Tagging a release, the release workflows | @ref[docs/RELEASING.md] |
| Toolchain, local setup | @ref[docs/SETUP.md] |
| The marker syntax as users see it | @ref[crates/context-anchors/templates/ANCHR.md] |
