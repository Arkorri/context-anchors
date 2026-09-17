---
title: Architecture
description: How the code realises the design, as a pipeline, a codemap of every module, the key types, and the layer invariants. Read before adding a module, a stage, or a consumer of the core.
tags: [architecture, core, cli, lsp]
---

# Architecture

<!-- refs -->
@ref[docs/DESIGN.md as Design]
@ref[crates/anchr-core/src/text/text.rs#FileAnalyzer as FileAnalyzer]
@ref[crates/anchr-core/src/text/language/language.rs#LanguageRegistry as LanguageRegistry]
@ref[crates/anchr-core/src/index/index.rs#Index as Index]
@ref[crates/anchr-core/src/index/index.rs#Site as Site]
@ref[crates/anchr-core/src/index/index.rs#FileRecord as FileRecord]
@ref[crates/anchr-core/src/tree/tree.rs#FileTree as FileTree]
@ref[crates/anchr-core/src/marker/path/path.rs#RelPath as RelPath]
@ref[crates/anchr-core/src/root/root.rs#FilePath as FilePath]
@ref[crates/anchr-core/src/resolve/resolve.rs#Resolver as Resolver]
@ref[crates/anchr-core/src/text/source/source.rs#SymbolTable as SymbolTable]
@ref[crates/anchr-core/src/diagnostic/diagnostic.rs#Report as Report]
@ref[crates/anchr-core/src/diagnostic/diagnostic.rs#DiagnosticKind as DiagnosticKind]
@ref[crates/anchr-core/src/check/check.rs#Workspace as Workspace]
@ref[#cli/check as Check]
@ref[#cli/coverage as Coverage]
@ref[#cli/backrefs as Backrefs]
@ref[#cli/rename as Rename]
@ref[#cli/init as Init]
@noref[src/Foo.ts, foo.ts, .claude/skills/**]

## 1. Bird's-eye view
<!-- @anchor[design/architecture] -->

Three container parsers, one marker lexer, one index. Everything is derived from the files on
every run; nothing is persisted.

```text
config ─► RootSet ─► scan each present root ─► FileScan per file ─► Index per root
                                                                        │
              Report ◄── group by cause ◄── Resolver ◄──────────────────┘
```

| Container | Parser | Where markers may appear |
|---|---|---|
| Markdown | pulldown-cmark | everything outside code blocks and inline code |
| Source code | tree-sitter | comment nodes, minus backtick spans |
| Plain text | none | the whole file |

Never write a markdown parser: only the container AST can tell prose from a fenced example, and
markers in examples must not be checked. Marker lexing needs no AST; the language is regular
(@[Design] §3), so one regex over the container's text regions suffices.

The core is LSP-shaped. Diagnostics, go-to-definition, find-references, rename, and document
symbols all map onto the same index, so the CLI, CI, and the language server are thin adapters
over one @[Workspace].

## 2. Crates and invariants

Two crates. `anchr-core` (@ref[crates/anchr-core/]) holds everything testable; `context-anchors`
(@ref[crates/context-anchors/], binary `anchr`) is the adapter: argument parsing, rendering,
exit codes, the LSP loop.

Invariants a change must not break:

- **Findings are data, not errors.** A broken reference, a malformed marker, an unreadable file
  is a diagnostic inside @[Report]. `Err` means the tool itself failed (bad config, unreadable
  root) and maps to exit 2. One bad file never aborts a check.
- **The scan is the sole authority on what exists.** Path resolution consults the @[FileTree] the
  walk produced, never the disk, so gitignored and config-ignored paths are missing and a local
  run agrees with a clean checkout. The one exception is a symlink whose target leaves the root,
  classified with `fs::metadata` for existence only.
- **No persisted state.** The @[Index] is rebuilt every run and is incremental only in memory,
  for the LSP.
- **The core has no `anyhow`.** Every fallible core function returns a `thiserror` enum; the
  binary adds context and maps to exit codes.
- **One @[Workspace] serves every consumer.** @[Check], @[Backrefs], @[Rename], @[Coverage], and
  the LSP all run against it; none re-scans on its own.
- **Unverifiable never renders as valid**, and the other four invariants of @[Design] §6.

## 3. Codemap

One line per module. Each module directory holds `<name>.rs` and `<name>_tests.rs`.

### anchr-core

| Module | Responsibility |
|---|---|
| @ref[crates/anchr-core/src/span/span.rs] | `ByteSpan` (half-open byte offsets), `LineCol` (1-based, for display), `LineIndex`, protocol positions in UTF-8 or UTF-16 |
| @ref[crates/anchr-core/src/root/root.rs] | `RootName`, @[FilePath] (any UTF-8 root-relative file), `Root`, `RootStatus`, `RootSet` |
| @ref[crates/anchr-core/src/config/config.rs] | @ref[anchr.toml] discovery, defensive parse into `Config`, per-root loading |
| @ref[crates/anchr-core/src/marker/marker.rs] | the marker language: `Marker`, `MarkerPayload`, `MalformedMarker`; children `id`, `alias`, `symbol`, `path` (@[RelPath]), `target` (`parse_target`), `noref`, `lex` |
| @ref[crates/anchr-core/src/text/text.rs] | `TextRegions`, `Container`, @[FileAnalyzer]; children `markdown`, `source` (@[SymbolTable]), `language` (@[LanguageRegistry]) |
| @ref[crates/anchr-core/src/scan/scan.rs] | walks one root in parallel and lexes every opted-in file |
| @ref[crates/anchr-core/src/tree/tree.rs] | @[FileTree]: what the scan enumerated, exact-path lookup, lexical symlink redirection |
| @ref[crates/anchr-core/src/index/index.rs] | @[Index]: per-root markers keyed by file, anchors by id, per-file alias tables, `backrefs` |
| @ref[crates/anchr-core/src/resolve/resolve.rs] | does a target exist: `Resolution`, `Unresolved`, `Unverified`, @[Resolver]; children `path`, `symbol` |
| @ref[crates/anchr-core/src/suggest/suggest.rs] | did-you-mean over a candidate set, rustc's rule |
| @ref[crates/anchr-core/src/diagnostic/diagnostic.rs] | @[DiagnosticKind], `Locations`, `Diagnostic`, `Summary`, @[Report], `ReportBuilder` |
| @ref[crates/anchr-core/src/check/check.rs] | @[Workspace] (load every present root, index them) and `check`, the guarantee |
| @ref[crates/anchr-core/src/noref/noref.rs] | `NoRefSet`: the strings an author declared are not references |
| @ref[crates/anchr-core/src/coverage/coverage.rs] | the advisory scanner: candidate tokens, grouping by token, proposals; children `token`, `linguist` (generated) |
| @ref[crates/anchr-core/src/edit/edit.rs] | byte-span `TextEdit`s that refuse a file that changed since it was scanned |
| @ref[crates/anchr-core/src/rename/rename.rs] | plan and apply an anchor-id rename, or a file-local alias rename |

### context-anchors

| Module | Responsibility |
|---|---|
| @ref[crates/context-anchors/src/main.rs] | dispatch and exit codes: 0 clean, 1 errors, 2 tool failure |
| @ref[crates/context-anchors/src/cli/cli.rs] | clap derive; every subcommand carries an `@anchor[cli/<name>]` |
| @ref[crates/context-anchors/src/commands/commands.rs] | shared helpers (`discover`, `Outcome`) and one child per subcommand |
| @ref[crates/context-anchors/src/render/render.rs] | `human` (rustc-shaped), `json` (versioned DTOs), `github` (the human report plus one workflow-command annotation per location), `coverage` renderers |
| @ref[crates/context-anchors/src/lsp/lsp.rs] | the synchronous stdio server; children `server` (handlers), `convert` (spans and paths to positions and URIs) |

## 4. Stages

### Text regions

@[FileAnalyzer] is per thread: tree-sitter's `Parser` is `!Sync` and must not be built per file;
one parser switches language per file. `Container` is a closed selector enum chosen by extension
from `[containers]` merged over defaults; an unknown extension means the file is not scanned at
all (opt-in applies to files too).

- **Markdown**: `into_offset_iter` yields byte ranges. Collect the *excluded* ranges (code blocks
  and inline code); the included regions are the complement. Working from exclusions rather than
  from `Text` events is deliberate: pulldown-cmark splits text at escapes, entities, and
  brackets, and `@ref[a]` must not be lost to a split. The lexer always runs on the source slice,
  never on the unescaped event text. HTML is included, so `<!-- @anchor[x] -->` is the way to
  place an anchor that does not render. A narrow `catch_unwind` wraps the iterator because it
  panics on some malformed documents (pulldown-cmark#1129); a panic becomes an unverified skipped
  file.
- **Source**: parse, run the language's comment query, take each capture's byte range, and carve
  out same-line backtick spans so doc comments can show example markers. Parse errors do not fail
  the file; comments still extract. @[LanguageRegistry] is a table of extension to grammar plus a
  comment query and a declaration query (the grammar's `tags.scm` concatenated with per-language
  supplements, because `tags.scm` coverage is uneven). It is built once per run and returns
  `Result`, because query compilation can fail on grammar drift and a static initializer would
  have to `expect`.
- **Plaintext**: one region, the whole file.

### Lexing

One regex in a `LazyLock`, over each region's source slice:

```text
@(anchor|ref|noref|)\[(?:([^\[\]\n]*)\]|)
```

An empty kind is an alias use. The body class excludes `[`, so an unclosed opener cannot swallow
the next marker. The empty-body alternative catches an opener with no closer on the line. A
post-filter rejects a match whose preceding *character* is alphanumeric or `_`, so an email-like
token is never a marker, and a preceding backslash escapes one. Bodies go to the newtype parsers
(`AnchorId::parse`, `parse_target`, `parse_noref_body`); failures become `MalformedMarker`s with
a reason. A marker records spans only; file identity is added by the index.

### Scan
<!-- @anchor[code/scan] -->

`ignore::WalkBuilder` with `hidden(false)`, git ignore files on, `require_git(true)`, ripgrep
`.ignore` files off, `follow_links(false)`, and one `filter_entry` that prunes `.git` at any depth
and anything `[ignore] paths` matches. `require_git(true)` makes `.gitignore` behave exactly as
git does: honoured only inside a repository, parent files stopping at the nearest `.git`. Hidden
files are walked because `.claude/skills/**` and `.github/workflows/*.yml` are exactly the
documentation this tool checks. `[scan] include` is a `globset` post-filter in the visitor, never
a walker override, because overrides take precedence over ignore files and would silently
un-ignore gitignored files.

`build_parallel` gives a visitor per thread, each owning a @[FileAnalyzer] and a channel sender.
Every file and symlink the walk yields is recorded in the @[FileTree] before any gating, so
@ref[Cargo.lock] is in the tree though never lexed. Then, per file with a container: size check
against `max-file-bytes` (default 2 MiB), read, UTF-8 validation, lex, send. The main thread folds
the channel into scans, skipped files, walk problems, and the set of extensions seen. External
roots are scanned `AnchorsOnly`: their references are discarded because the user is checking
their own root, and a duplicate anchor there is unverified, not an error, since it cannot be fixed
from here. Third-party walker behaviour the design depends on is pinned by
@ref[crates/anchr-core/tests/walker_behaviour.rs].

### Indexing

Per root, in memory. `files: HashMap<FilePath, FileRecord>` is the single owner of marker data;
`anchors_by_id` is derived and rebuilt for a file inside `update_file` and `remove_file`, so the
two cannot drift. A @[FileRecord] holds the file's markers, malformed markers, `LineIndex`, and
its alias table, built from that file's declarations alone (@[Design] §3). @[Site] (root, path,
span, region) is built here, not by the lexer. `backrefs` chains direct references with bound
alias uses.

### Resolution

Root selection happens once, before dispatch on target kind: no prefix is the current root; an
undeclared name is `Unresolved::RootUndeclared` (a typo, with a suggestion); a declared but absent
root is `Unverified::RootAbsent`.

- **Path**: a lookup in the @[FileTree]. A file exists when the walk enumerated it; a directory
  exists when something beneath it was enumerated, so an empty or fully ignored directory does
  not. Keys are the exact bytes the walker reported, so `src/Foo.ts` fails against `foo.ts` on
  macOS exactly as in Linux CI. A symlink entry is redirected lexically, at most
  `MAX_SYMLINK_HOPS` times. A trailing `/` additionally requires a directory
  (`PathNotDirectory`; `PathNotFile` for the symbol case). A missing path that is on disk gets a
  note naming the ignore line that removed it.
- **Symbol**: the path must exist; the joined path is canonicalised and must `starts_with` the
  canonical root (else `PathEscapesRoot`, since this is a read); the language is looked up by
  extension (none is `NoGrammar`); parsing runs under a wall-clock budget (`ParseTimeout`); the
  declaration query fills a @[SymbolTable] of name to declaration spans, capped at 100k
  (`SymbolTableTruncated` discards the table rather than returning a partial one). Found is
  resolved; not found in a tree with errors is `ParseErrors`, because a declaration inside an
  ERROR node is invisible and reporting it missing would be a false positive; not found in a clean
  tree is `SymbolMissing`. The guarantee is exactly "a declaration with this name exists in this
  file", at any nesting depth. A per-run symbol cache means a file referenced forty times is
  parsed once.
- **Anchor**: `anchor_sites(id)` in the selected root. Duplicates still resolve; the duplicate is
  its own diagnostic at the anchor sites.

Resolution is single-threaded after the fold, so its caches are plain `HashMap`s.

### Diagnostics

@[DiagnosticKind] is the grouping key: it carries the cause and never a location, so one missing
anchor with twelve sites is one diagnostic. `Locations` is `Sites` for marker-level findings,
`Files` for file-level ones (skipped files), `Roots` for root-level ones. Severity is a function
of the kind. `ReportBuilder` sorts sites by (path, line), errors before unverified, then by site
count descending. Alias findings (`AliasUndeclared`, `AliasDuplicate`) are keyed by file because
the file is the scope. Suggestions follow rustc: case-insensitive exact match first, then
`osa_distance` within a third of the length, then the same on the last `/` segment; at most one.
`Report::has_errors` drives exit code 1; `--strict` or `[check] unverified = "error"` promotes
every unverified finding.

### Rendering and the CLI

The human renderer uses `annotate-snippets` (rustc's): per diagnostic, the title, the first site
as a source snippet with the span underlined, the remaining sites as a `--> path:line:col` list
capped at 40, then the suggestion or hint. Source is not retained in the index; the renderer
re-reads the one file per diagnostic and degrades to the list form if that fails. Output goes
through `anstream` for TTY detection and `NO_COLOR`. The JSON renderer serialises its own DTOs,
not the core types, under `"schema": 1`; locations carry path, 1-based line/col, byte span, and
region kind; unverified diagnostics carry a `hint` naming the fix. The GitHub renderer prints the
human report, then one `::error` or `::warning` line per location for GitHub's log parser: `title`
is the JSON code, the message is the kind's text with the suggestion and hint appended, and `file`
is relative to `GITHUB_WORKSPACE` (the working directory when unset), so a root outside it gets
no `file` and the location moves into the message. Every location is emitted; the 40-site cap is
a reading aid for the human form only, and `col` is the same 1-based byte column the JSON carries.
@[Coverage] has its own renderer: one group per (verdict, token), no snippets.

Subcommands and flags are documented by `anchr --help` and @ref[docs/guide/commands.md]. `PATHS`
filters which files' references and malformed markers are reported; indexing still covers the
whole root, and root-wide findings are always reported. @[Init] is the only command that writes without an
explicit flag: never overwrite without `--force`, print every path, be a no-op the second time; it
merges a hook into `.claude/settings.json` by read-modify-write of a JSON value that preserves
every key it does not own.

### Language server

`lsp-server` plus `ls-types`, synchronous, over stdio. Every message is a pure function of the
@[Workspace] plus the open documents, so there is no concurrency to manage; each message is
wrapped in `catch_unwind` so one panic does not kill the editor session. Full document sync;
`Workspace::update_file` re-lexes one document from editor text. Positions are UTF-8 when the
client offers it, UTF-16 otherwise, via `LineIndex`. Definition on an alias use returns the
target plus the declaration; rename disambiguates by cursor position between the anchor id and the
alias token. One caveat: an editor buffer for a gitignored file counts as present for that session;
the CLI is the CI truth. The connection is dropped before the stdio threads are joined, or the
writer thread never exits.

## 5. Cross-cutting

- **Config.** @ref[anchr.toml] is found by walking ancestors, else the nearest `.git`, else the
  working directory; a missing file means defaults. Every section is `deny_unknown_fields`,
  kebab-case, parsed into raw structs and validated once into `Config`, with `toml::Spanned` so
  semantic errors point at a line. The schema with every default is the template
  @ref[crates/context-anchors/templates/anchr.toml]; do not restate it elsewhere. External roots
  load their own `[scan]`, `[containers]`, and `[ignore] paths`; their `[roots]`, `[check]`, and
  `[ignore] tokens` are ignored, so root cycles are impossible. Config is parsed defensively but
  the roots it declares are trusted: pointing a root at `~` walks `~`.
- **Two path types.** @[RelPath] is the reference grammar's allowlisted path; @[FilePath] is any
  UTF-8 root-relative name the scan produced (`My Notes.md` must be scanned even though no
  reference can name it). The index is keyed by @[FilePath]; @[RelPath] converts into it for
  resolution. All paths are `camino` UTF-8; `std::path` appears only at OS boundaries.
- **Spans and positions.** Byte offsets internally; conversion to `u32` for JSON and LSP goes
  through `try_from` and `PositionOverflow`, never `as`.
- **Concurrency.** Immutable inputs (config, registry, root set) are shared by `&`. Per-file work
  is pure and runs on the walker's threads; results flow over a channel to a single-threaded
  fold. Resolution and rendering are single-threaded. The only statics are the lexer and
  tokeniser regexes.

## Rejected alternatives

- pulldown-cmark over comrak: byte ranges natively; comrak's positions are 1-based line/col and
  would need a second index.
- No persisted index over a gitignored cache: a full scan of a mid-size repo is well under a
  second, and a cache adds an invalidation surface with no correctness benefit.
- `root:` on every target kind over anchors only: one rule, no extra cost.
- annotate-snippets over miette or ariadne: rustc's renderer already does cause-to-N-sites and
  multi-file gutters; miette wants to own error types, ariadne defaults to char offsets.
- lsp-server plus ls-types over tower-lsp or tokio: a re-parse-whole-document server has no I/O
  concurrency to justify an async runtime; tower-lsp and lsp-types are unmaintained.
- The ignore walker over rayon: the walker already provides a thread per core with per-thread
  state; the post-walk phase is cheap.
- Running the tags query ourselves over tree-sitter-tags: one API for comments and declarations.
- `RelPath::parse` over path-clean: the component walk is the validation.
- Region complement over per-event lexing: an event split inside a marker would lose it.
- Existence from the scan tree over `read_dir`: build output in a gitignored directory resolved
  locally and failed on a clean checkout.
- One `[ignore]` table over five knobs (`.anchrignore`, `[scan] exclude`, `[coverage] exclude`,
  `[coverage] ignore`, `@noref`): two questions, two keys; the old trailing-slash prefix rule hid
  every `src/...` mention. `require_git(true)` in the same change, after a copy of this repository
  under `~/.claude` lost a directory to an unanchored line in a parent `.gitignore`.
- Bare code symbols as coverage candidates: a name has no single referent; of 48 such rows on
  this repository, one was a real miss. Symbols enter coverage through an alias declaration.
- Extensions from Linguist plus the root over a fixed list: every unresolvable coverage row had
  been a prose slash pair.
