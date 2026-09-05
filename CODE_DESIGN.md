# context-anchors — code-level design

**Status:** approved 2026-09-04
**Companion to:** `DESIGN.md` (what the tool is) and `DISTRIBUTION.md` (how it ships); this
document covers how the code is shaped. Where it deviates from those two, §12 says so.

## Context

`DESIGN.md` and `DISTRIBUTION.md` settle *what* `anchr` is: opt-in `@anchor[id]` / `@ref[target]`
markers in prose and code comments, batch-validated by `anchr check`, grouped-by-cause diagnostics
with an explicit *unverified* class, Rust single binary, LSP later. The repo has no code yet.

This document is the code-level design that turns that into a Rust workspace: crate layout, module
map, core types, the per-stage algorithms, the crate choices (reuse first), error/diagnostic
model, security posture, CLI surface, config schema, and test strategy. Open questions from
`DESIGN.md` §11 are answered inline where the code forces a decision. Question 4 (premise
validation) is skipped per instruction.

The two design docs are drafts: where research found a better option, this plan takes it and
records the deviation in §12. All milestones are built in quick succession before any release,
so the code is shaped for the end state rather than for an interim v1.

Governing principles for the whole build:
- Rust security best-practices checklist (corgea) — concrete rules in §10.
- Comments only where a competent reader would otherwise be wrong. Names carry the weight.
- Reuse proven crates; write from scratch only the marker lexer and the grouping/report logic,
  which is where the novelty actually is.

---

## 1. Workspace layout

Cargo workspace, two crates. Library holds everything testable; binary is a thin adapter.
This is the layout ruff/biome/rust-analyzer use and it is what lets `anchr lsp` (v1.1) reuse
the identical core without a second binary.

```
context-anchors/
  Cargo.toml                      # [workspace], shared [workspace.dependencies], lints
  deny.toml                       # cargo-deny: licenses, advisories, bans, sources
  rust-toolchain.toml
  crates/
    anchr-core/                   # lib: scan → lex → index → resolve → diagnostics
      src/
        lib.rs
        config.rs                 # Config schema (serde), discovery up the tree
        root.rs                   # Root, RootName, RootSet, RootStatus
        span.rs                   # ByteSpan, LineCol, LineIndex wrapper
        text/
          mod.rs                  # TextRegions (included byte ranges of a file)
          markdown.rs             # pulldown-cmark → excluded ranges → complement
          source.rs               # tree-sitter → comment nodes
          plaintext.rs            # whole file
          language.rs             # LanguageRegistry: ext → Language, comment kinds, TAGS query
        marker/
          mod.rs                  # Marker, AnchorId, RefTarget, parse_target()
          lex.rs                  # find markers in text regions; malformed detection
        scan.rs                   # walk a root (ignore crate) → FileScan per file, in parallel
        index.rs                  # Index: anchors by id, refs, per-file records; update/remove
        resolve/
          mod.rs                  # Resolver, Resolution, Unresolved/Unverified reasons
          path.rs
          symbol.rs               # tree-sitter tags query, per-run parse cache
          anchor.rs
        diagnostic.rs             # Diagnostic, DiagnosticKind, Severity, Report, grouping
        suggest.rs                # did-you-mean over candidate sets (strsim)
        check.rs                  # run_check(config, options) -> Report   (the one entrypoint)
    context-anchors/              # bin package; [[bin]] name = "anchr"
      src/
        main.rs                   # exit-code mapping only
        cli.rs                    # clap derive
        commands/check.rs
        commands/init.rs
        render/human.rs           # grouped text report, colors via anstream/owo-colors
        render/json.rs            # stable versioned schema
  tests/                          # in each crate; fixtures under crates/*/tests/fixtures
  anchr.toml                      # dogfood: this repo checks its own docs in CI
```

Package names: `anchr-core` (lib), `context-anchors` (bin → `anchr`). Matches `DISTRIBUTION.md` §1.

---

## 2. Core types (anchr-core)

All identifiers are newtypes so a path, an ID, a root name, and a symbol name can never be
mixed up at a call site. Validation happens at construction; the rest of the code trusts them.

```rust
// span.rs
pub struct ByteSpan { pub start: usize, pub end: usize }          // half-open, within one file
pub struct LineCol { pub line: u32, pub col: u32 }                // 1-based for display
pub struct LineIndex(line_index::LineIndex);                      // byte → LineCol, built once per file

// root.rs
pub struct RootName(String);            // `[A-Za-z0-9_-]+`; "" is never a RootName
pub struct Root { pub name: RootName, pub dir: Utf8PathBuf, pub config: Config }
pub enum RootStatus { Present(Root), Absent { name: RootName, declared_dir: Utf8PathBuf } }
pub struct RootSet { current: RootName, roots: BTreeMap<RootName, RootStatus> }

// marker/mod.rs
pub struct AnchorId(String);            // segments `[A-Za-z0-9_][A-Za-z0-9_.-]*` joined by `/`
pub struct RelPath(Utf8PathBuf);        // root-relative, normalized; allowlisted segment charset
pub struct SymbolName(String);          // unqualified identifier; `Foo::bar` / `Class.method` rejected
pub enum PathExpectation { Any, Directory }   // `Directory` when the target ended in `/`

pub enum RefTarget {
    Path   { root: Option<RootName>, path: RelPath, expects: PathExpectation },
    Symbol { root: Option<RootName>, path: RelPath, name: SymbolName },
    Anchor { root: Option<RootName>, id: AnchorId },
}

pub struct Site { pub root: RootName, pub path: RelPath, pub span: ByteSpan, pub region: RegionKind }
pub enum RegionKind { Prose, Comment, Whole }

pub enum Marker {
    Anchor { id: AnchorId, site: Site, id_span: ByteSpan },                        // id_span: the bytes `rename` rewrites
    Ref    { target: RefTarget, site: Site, body_span: ByteSpan, id_span: Option<ByteSpan> },
}

pub enum MalformedMarker {
    Unclosed        { kind: MarkerKind, site: Site },
    EmptyBody       { kind: MarkerKind, site: Site },
    InvalidAnchorId { raw: String, site: Site, reason: IdError },
    InvalidTarget   { raw: String, site: Site, reason: TargetError },
}
```

`Option<RootName>` on every target variant: the `root:` prefix is allowed on all three target
kinds, not only on `#id`. `DESIGN.md` §4 only shows `root:#id`, but the grammar is
`[root:]target` and making paths/symbols cross-root capable costs nothing and keeps one rule (§12 item 3).

### Target grammar (parse_target)

```
target      := [root ":"] body
body        := "#" anchor_id                      -> Anchor
             | rel_path "#" symbol_name           -> Symbol
             | rel_path ["/"]                     -> Path   (trailing "/" ⇒ PathExpectation::Directory)
root        := [A-Za-z0-9_-]+
anchor_id   := segment ("/" segment)*
segment     := [A-Za-z0-9_] [A-Za-z0-9_.-]*
rel_path    := path_seg ("/" path_seg)*
path_seg    := one or more printable non-whitespace chars excluding  [ ] # : \  and control chars;
               "." and ".." segments rejected; leading "/" rejected
symbol_name := [A-Za-z_$] [A-Za-z0-9_$]*        (unqualified; "::" or "." ⇒ TargetError::QualifiedSymbol)
```

Hand-written, ~60 lines, no regex needed for this level. Two characters are *reserved* by the
grammar rather than justified by platform rules: `:` after a root-shaped prefix introduces a
root, and `#` anywhere introduces a name. A file whose name contains either cannot be referenced
by path; the grammar docs say so. Split on the first `:` only if the prefix matches `root` and
what follows is non-empty; then split on the first `#`.

Qualified symbols are rejected, not silently matched on their last segment: the guarantee is
file-scoped ("a declaration named X exists in this file"), and `Foo::bar` would otherwise be a
guaranteed `SymbolMissing` with a useless suggestion. The rejection message states the
file-scoped rule.

`RelPath::parse` is an allowlist, like the other newtypes: it walks components, rejects
`RootDir`/`Prefix`/`ParentDir`/`CurDir`, checks each segment's charset, and never touches the
filesystem. `parse_target` records `PathExpectation::Directory` *before* normalization, because
`Utf8PathBuf` drops trailing separators. Resolution joins onto `Root.dir`; the normalized form is
what guarantees the join cannot escape the root (§10).

`parse_target` also returns the byte span of the ID portion for `#id` / `root:#id` targets so
`rename` (step 10) rewrites exactly the ID bytes and nothing else.

---

## 3. Pipeline stages

```
RootSet (config) ─► scan each present root ─► FileScan{markers, malformed} ─► Index
                                                                              │
                     Report ◄── group ◄── Diagnostics ◄── Resolver ◄──────────┘
```

Single entrypoint: `check::run_check(root_set, options) -> Result<Report, CheckError>`.
`CheckError` is tool failure (bad config, unreadable root) → exit 2. Broken references are
*data* in `Report`, never `Err`.

### 3.1 Text regions (which bytes may contain markers)

`text::TextRegions` is a sorted `Vec<(ByteSpan, RegionKind)>` of *included* ranges. The lexer
only ever runs over included ranges, so the container is the only thing that decides "prose vs
example". `Container` is a pure selector enum (closed set, no trait object); the work happens in
a per-thread `FileAnalyzer`, because tree-sitter's `Parser` is `!Sync` and must not be
constructed per file:

```rust
pub enum Container { Markdown, Source(&'static LanguageSpec), Plaintext }
impl Container { pub fn for_path(path: &Utf8Path, config: &Config, registry: &LanguageRegistry) -> Option<Self> }

pub struct FileAnalyzer<'r> { parser: Parser, registry: &'r LanguageRegistry }
impl FileAnalyzer<'_> {
    pub fn text_regions(&mut self, container: Container, source: &str) -> Result<TextRegions, ContainerError>;
    pub fn scan(&mut self, path: RelPath, source: &str) -> FileScan;                 // regions + lex
    pub fn symbols(&mut self, spec: &LanguageSpec, source: &str) -> Result<SymbolTable, ContainerError>;
}
```

One `Parser` per analyzer suffices: `set_language` switches per file. Both the scan visitors
(§3.3) and the resolver's symbol lookups (§3.5) go through the same type.

**Markdown** — `pulldown-cmark` with `Parser::new_ext(src, opts).into_offset_iter()`, which
yields `(Event, Range<usize>)` **byte** ranges into the source (verified; comrak's sourcepos is
1-based line/col and markdown-rs offsets are char-based, both of which would need conversion).
Collect *excluded* ranges: `Start(CodeBlock)..End(CodeBlock)` and inline `Code`. Included =
complement of excluded over `0..len`, kind `Prose`. Working from the exclusion complement rather
than from individual `Text` events is deliberate: pulldown-cmark splits text runs at escapes,
entities, and around `[`/`]` (which look like reference-link syntax), and `@ref[a]` must not be
lost to a split. We always lex the *source slice*, never the event's unescaped `CowStr`, so
offsets stay aligned. HTML blocks/inline HTML (`<!-- @anchor[x] -->`) are *included* — an HTML
comment is the natural way to place an anchor that does not render. Options: `ENABLE_TABLES |
ENABLE_FOOTNOTES | ENABLE_STRIKETHROUGH | ENABLE_TASKLISTS | ENABLE_HEADING_ATTRIBUTES`. Not
`ENABLE_SMART_PUNCTUATION` (rewrites text) and not `ENABLE_WIKILINKS` (`[[..]]` would interact
with marker brackets).

**Source** — `tree-sitter` 0.27. Parse with the registered `Language`, then run the language's
compiled comment query (`[(line_comment) (block_comment)] @comment` for Rust, `(comment) @comment`
for JS/TS/Python/Go) with `QueryCursor` and take each capture's `byte_range()`. Region kind
`Comment`. Using the same query mechanism as symbol lookup (§3.5) means one API for both jobs.
Parse errors from tree-sitter do not fail the file: comments still extract from an
error-tolerant tree. A file whose extension has no registered language is not a source
container at all (see registry below). `Parser` is `!Sync`, so one lives per walker thread
(§3.3); `Query` objects are compiled once per language and shared.

**Plaintext** — one region `0..len`, kind `Whole`. Answers `DESIGN.md` §11 Q3 for v1: a
plaintext container cannot carry documentation about the marker syntax, and that is
acceptable; the escape hatch is to write such docs in markdown. Note this in the user docs.

**LanguageRegistry** (`text/language.rs`): table
`ext → LanguageSpec { name, language: Language, comment_query: Query, declaration_query: Option<Query> }`
for the core bundle: TypeScript and TSX (two parsers from one crate), JavaScript, Python, Rust,
Go; markdown is handled by pulldown. Constructed by `LanguageRegistry::new() -> Result<Self,
RegistryError>` once in `run_check` and passed by `&`, not in a `LazyLock`: `Query::new` returns
`Result` (ABI or query-syntax drift is a real runtime failure) and a static initializer would
have to `expect`. A unit test asserts construction succeeds so drift fails CI.

`declaration_query` is the grammar crate's `TAGS_QUERY` (tree-sitter's `tags.scm`, verified
exported by all five) *concatenated with* a per-language `supplementary_declarations` string,
because `tags.scm` coverage is uneven — TypeScript's is short and is not expected to capture
`type_alias_declaration`, `enum_declaration`, or `export const f = () => {}`, the dominant
declaration form in TS. The per-kind fixture list in §9 is written before the supplementary
queries so gaps surface as failing tests, not as false `SymbolMissing` errors. Grammar crates
depend only on `tree-sitter-language`, so the runtime version is pinned independently
(`LANGUAGE.into()`). Adding a language = one table row + one dependency. Known caveat:
`tree-sitter-typescript` is the stalest grammar (late 2024); newer TS syntax parses with errors,
which §3.5 turns into an *unverified* outcome rather than a false error.

**Container selection**: by extension, from `Config.containers` merged over defaults.
Extension not in any list → file is not scanned at all (opt-in principle applies to files as
well as markers).

### 3.2 Marker lexing (`marker/lex.rs`)

The marker language is regular. One compiled `regex::Regex` in a `LazyLock`:

```
@(anchor|ref)\[(?:([^\]\n]*)\]|)
```

The empty alternative catches an opener with no closer on the same line → `Unclosed`. A
post-filter rejects matches whose preceding *character* (`source[..start].chars().next_back()`,
not the preceding byte, so `é@ref[x]` and `e@ref[x]` behave the same) is alphanumeric or `_`, so
`foo@ref[...]` inside an email-like token is not a marker. Because the lexer runs over the source
slice and not the rendered text, a markdown backslash escape `@ref\[x\]` is never lexed; this is
the documented way to show a literal marker in prose outside a code fence. For each region, run the regex on `&source[span]`, offset
match positions by `span.start`. Body goes to `AnchorId::parse` or `parse_target`; failures
become `MalformedMarker`. Multiple markers per line are naturally supported.

Output per file: `FileScan { path, markers: Vec<Marker>, malformed: Vec<MalformedMarker>, line_index: LineIndex }`.

### 3.3 Scan (`scan.rs`)

For a `Root`: `ignore::WalkBuilder::new(root.dir)` with `.hidden(true)`, `.git_ignore(true)`,
`.require_git(false)` (so `.gitignore` is honored in non-git roots like `~/.claude`),
`.follow_links(false)`, `.add_custom_ignore_filename(".anchrignore")`, and an `OverrideBuilder`
holding **only** `config.scan.exclude` as `!` globs. `include` is *not* an override: `ignore`
consults overrides before ignore files and returns on any override match, so a whitelist glob
would silently un-ignore gitignored files (`CHANGELOG.md`, `*.generated.ts`). Instead `include`
is compiled to a `globset::GlobSet` (`literal_separator(true)`) and applied as a post-filter in
the visitor, after the walker has already applied `.gitignore`. A two-line integration test
pins this: a gitignored `.md` file must not be scanned.

`build_parallel()` gives a thread per core with a per-thread visitor. Each visitor owns a
`FileAnalyzer` (§3.1) and a clone of an `mpsc::Sender<ScanOutcome>`. Per file:
`symlink_metadata` size check first (> `config.scan.max_file_bytes`, default 2 MiB →
`SkippedFile::TooLarge`), then `std::fs::read` + UTF-8 validation (non-UTF-8 →
`SkippedFile::NotUtf8`), `analyzer.scan(path, source)`, send. The main thread drains the channel
into `Vec<FileScan>` + `Vec<SkippedFile>`. No `rayon`: the walker already provides the
parallelism, and channel-to-single-reducer avoids a shared `Mutex<Vec>`. Paths convert to
`Utf8PathBuf` at this boundary; a non-UTF-8 file name is `SkippedFile::NonUtf8Path`.

**External roots are scanned in `ScanMode::AnchorsOnly`.** Their refs and malformed markers are
discarded: the user is checking *their* root, and an external root's own cross-root refs could
not resolve anyway (its `[roots]` table is not loaded). Only its anchors are indexed. A
duplicate anchor ID inside an external root is reported as *unverified* (`ExternalDuplicate`),
not as an error, because it cannot be fixed from here.

A skipped file (too large, not UTF-8) may contain anchors that are therefore unindexed, which
can surface elsewhere as `AnchorMissing`. The `FileTooLarge` / `FileNotUtf8` diagnostics say so
explicitly and name `max-file-bytes` as the knob, so the cause is visible in the same report.

### 3.4 Index (`index.rs`)

Per root, in memory, derived:

```rust
pub struct Index {
    files: HashMap<RelPath, FileRecord>,        // single source of truth: markers, malformed, LineIndex
    anchors_by_id: HashMap<AnchorId, Vec<Site>>, // derived secondary index; len > 1 ⇒ DuplicateAnchor
}
pub struct FileRecord { markers: Vec<Marker>, malformed: Vec<MalformedMarker>, line_index: LineIndex }
impl Index {
    pub fn from_scans(scans) -> Self;
    pub fn update_file(&mut self, scan: FileScan);   // replaces the FileRecord, re-derives its anchors
    pub fn remove_file(&mut self, path: &RelPath);
    pub fn anchor_sites(&self, id) -> &[Site];
    pub fn refs(&self) -> impl Iterator<Item=(&RefTarget, &Site)>;         // iterates files
    pub fn backrefs(&self, target) -> impl Iterator<Item=&Site>;           // step 10 command, free now
}
```

`files` is the only owner of marker data; `anchors_by_id` is rebuilt for a file inside
`update_file`/`remove_file` so the two can never drift.

**No persistent cache in v1.** `DESIGN.md` §7 says "incremental, gitignored"; the scan is
embarrassingly parallel and tree-sitter is fast enough that a full rebuild on a mid-size repo
is well under a second, and an on-disk cache adds an invalidation surface with no correctness
benefit ("never authoritative"). The `Index` API above is already incremental for the LSP's
in-memory needs. If measurement later says otherwise, persistence goes in a cache dir
(`directories` crate) keyed by root path, *not* inside the repo, so nothing needs gitignoring
(§12 item 2).

### 3.5 Resolution (`resolve/`)

```rust
pub enum Resolution {
    Resolved,
    Unresolved(Unresolved),
    Unverified(Unverified),
}
pub enum Unresolved {
    PathMissing  { root: RootName, path: RelPath },
    SymbolMissing{ root: RootName, path: RelPath, name: SymbolName },
    AnchorMissing{ root: RootName, id: AnchorId },
    RootUndeclared { name: RootName },                        // typo in prefix → error
}
pub enum Unverified {
    RootAbsent   { name: RootName, declared_dir: Utf8PathBuf },   // declared, not on disk
    NoGrammar    { extension: String },
    NoSymbolQuery{ language: &'static str },
    ParseErrors  { path: RelPath, language: &'static str },       // symbol absent from an error-bearing tree
    ParseTimeout { path: RelPath },                               // tree-sitter progress budget exceeded
    SymbolTableTruncated { path: RelPath },                       // declaration cap hit; table discarded
    ExternalDuplicate { root: RootName, id: AnchorId },
    FileNotUtf8  { path }, FileTooLarge { path, bytes },
}
```

`Resolver` holds `&RootSet`, `&HashMap<RootName, Index>`, its own `FileAnalyzer`, a
`DirectoryListingCache`, and a per-run `symbol_cache: HashMap<(RootName, RelPath), SymbolTable>`
so a file referenced by 40 refs is parsed once. Resolution runs single-threaded after the fold
(it is cheap relative to the scan), so these caches need no synchronization.

**Root selection happens once, before dispatch on target kind**, so all three variants share
one rule: `None` ⇒ current root; a name not in `RootSet` ⇒ `Unresolved::RootUndeclared` (a typo,
with a suggestion from the declared names); `RootStatus::Absent` ⇒ `Unverified::RootAbsent`;
otherwise resolution proceeds against that root's dir and index.

- **Path**: exact-name, component-by-component lookup against a `read_dir` listing of each
  parent, cached per directory for the run (`DirectoryListingCache`, shared with the sibling
  suggestions in §3.6). Not `symlink_metadata`: on a case-insensitive filesystem (macOS default)
  `@ref[src/Foo.ts]` would resolve against `foo.ts` and then fail in Linux CI, breaking invariant
  5. Every component present ⇒ Resolved. A trailing `/` in the target additionally requires the
  final entry to be a directory. Directory refs stay (§11 Q1: free, so keep).
- **Symbol**: path must exist (else `PathMissing`); the joined path is canonicalized and must
  `starts_with` the canonical root (else `PathEscapesRoot`, since this is a read); registry
  lookup by extension (none ⇒ `NoGrammar`; no declaration query ⇒ `NoSymbolQuery`); parse with
  `parse_with_options` and a progress callback that aborts past a wall-clock budget (⇒
  `Unverified::ParseTimeout`); run `declaration_query` with `QueryCursor::matches` (a
  `StreamingIterator` in tree-sitter 0.27, so `while let Some(m) = matches.next()`); per match,
  keep it only if it has a capture whose name starts with `definition.` (a per-match filter;
  there is no clean pattern-to-capture API for `disable_pattern`), and record the `@name`
  capture's text and `byte_range()` into `SymbolTable { declarations: HashMap<String,
  Vec<ByteSpan>>, has_parse_errors: bool }` — spans are what go-to-definition (step 9) needs.
  If the declaration cap (100k) is hit the table is discarded and the outcome is
  `Unverified::SymbolTableTruncated`, never a truncated table that yields false misses.
  Outcome: name found ⇒ Resolved; not found and `root_node().has_error()` ⇒
  `Unverified::ParseErrors { path, language }` with a hint about grammar staleness (a declaration
  inside an ERROR node is invisible to the query, and reporting that as missing would be a false
  positive); not found and clean tree ⇒ `SymbolMissing`. Guarantee is exactly `DESIGN.md` §7's: *a declaration with this name
  exists in this file*, any nesting depth. Running the query ourselves (~40 lines) rather than
  pulling `tree-sitter-tags` keeps one API for comments and declarations.
- **Anchor**: `index.anchor_sites(id).is_empty()` in the selected root ⇒ `AnchorMissing`.
  Duplicated IDs still resolve (the duplicate is its own error at the anchor sites, or an
  `ExternalDuplicate` unverified finding if the root is external).

Answers §11 Q2, more broadly than asked: every unverified outcome (absent root included) is
non-blocking by default; `--strict`, or `check.unverified = "error"` in config, promotes *all*
unverified findings to errors. CI users read `--strict` as "fail on anything you could not
check", so the flag means exactly that rather than a single-cause toggle.

### 3.6 Diagnostics and grouping (`diagnostic.rs`)

```rust
pub enum Severity { Error, Unverified }
pub struct Diagnostic { pub kind: DiagnosticKind, pub locations: Locations, pub suggestion: Option<String> }
pub enum Locations { Sites(Vec<LocatedSite>), Files(Vec<(RootName, RelPath)>) }   // marker-level vs file-level
pub struct LocatedSite { pub site: Site, pub line_col: LineCol }   // resolved from FileRecord.line_index at construction
pub enum DiagnosticKind {                      // Severity is a function of the kind; the key never contains a Site
    AnchorMissing{root, id}, DuplicateAnchor{id}, PathMissing{root, path}, SymbolMissing{root, path, name},
    RootUndeclared{name}, Malformed{kind: MarkerKind, reason: MalformedReason, raw: String},   // Error
    RootAbsent{..}, NoGrammar{extension}, NoSymbolQuery{language}, ParseErrors{..}, ParseTimeout{..},
    SymbolTableTruncated{..}, ExternalDuplicate{..}, FileNotUtf8, FileTooLarge                // Unverified
}
pub struct Report { pub diagnostics: Vec<Diagnostic>, pub summary: Summary }
pub struct Summary { refs_checked, refs_resolved, errors, unverified, anchors, files_scanned }
```

Grouping key is the `DiagnosticKind` value itself (derive `Eq + Hash`; it carries the *cause*
and never a location): one `AnchorMissing{auth/flow}` with twelve sites, not twelve
diagnostics; all `Unclosed` `@ref[` markers in one group. File-level findings use
`Locations::Files`. Sites sorted by (path, line). Errors sorted before unverified, then by
location count desc. `Report::has_errors()` drives exit code 1.

Suggestions (`suggest.rs`): rustc's rule, which fits short identifiers better than Jaro. Over
the candidate set (all anchor IDs in the target root; all symbol names in the target file;
sibling entries of the missing path's parent directory): first a case-insensitive exact match,
then `strsim::osa_distance` (restricted Damerau-Levenshtein) accepted when
`distance ≤ max(len, 3) / 3`, then for hierarchical IDs the same on the last `/` segment. At
most one suggestion, the lowest distance; never mutates.

### 3.7 Rendering (binary crate)

- **Human** (`render/human.rs`): `annotate-snippets` 0.12, which is rustc's own renderer and is
  built around exactly the cause→N-sites shape: one titled `Group` holding several `Snippet`s,
  each with its own path and byte-span annotations. Output goes through `anstream` for TTY
  detection and `NO_COLOR`. Per diagnostic: title = cause (`unknown id \`auth/flow\``), the first
  site rendered as a source snippet with the marker span underlined, the remaining sites as a
  compact `path:line:col` list (twelve full snippets would bury the report), and the suggestion
  as a `help:` footer. Source text is not retained in the `Index`; the renderer re-reads the one
  file per diagnostic it renders as a snippet and degrades to the `path:line:col` form if that
  read fails (the file changed or vanished between scan and render). Line/col for every site
  comes from `LocatedSite`, so the list form never needs the source. Multi-file gutter alignment is more work than it looks, which is why this
  is borrowed rather than written; miette wants to own error types and ariadne defaults to char
  offsets, so neither fits.
- **JSON** (`render/json.rs`): `serde` on a dedicated `JsonReport` DTO (not the core types, so
  the wire format is decoupled), with `"schema": 1`. Locations carry path, 1-based line/col,
  byte span, and region kind. Unverified diagnostics include a `hint` string that names the fix
  ("install the `full` build or add a grammar for `.ex`" per `DISTRIBUTION.md` §6).

Exit codes: 0 clean (unverified may be present), 1 errors, 2 tool failure.

---

## 4. Config (`config.rs`)

`anchr.toml`, discovered with `cwd.ancestors().find(|d| d.join("anchr.toml").is_file())` (or
`--root`), falling back to the nearest ancestor containing `.git`, then cwd. Missing file ⇒
defaults. `#[serde(deny_unknown_fields, rename_all = "kebab-case")]` on every struct,
`#[serde(default)]` on every section; syntax errors surface with `toml::de::Error`'s byte span
and built-in caret rendering (exit 2). Fields whose *values* are validated after parse
(root dirs, globs, extensions) are wrapped in `toml::Spanned<T>` so semantic errors point at the
offending line without a second parse.

```toml
[root]
name = "context-anchors"    # optional; defaults to the directory name; must be a valid RootName

[roots]                     # name → directory; `~` expanded; relative to this file
claude = "~/.claude"

[scan]
include = ["**/*.md", "**/*.txt", "**/*.{ts,tsx,js,jsx,py,rs,go}"]   # default shown
exclude = []                # added to .gitignore semantics, never replaces them
max-file-bytes = 2097152    # validated ≤ u32::MAX (line-index offsets are u32)

[containers]
markdown  = ["md", "markdown"]
plaintext = ["txt"]
# source containers come from the language registry; not user-extensible in v1

[check]
unverified = "report"       # or "error" (same as --strict)
```

The current root needs a `RootName` for every `Site`; `[root] name` provides it, defaulting to
the directory's basename (validated; an invalid basename is a config error that names the fix).

External roots load *their own* `anchr.toml` for `[scan]`/`[containers]` if one exists;
otherwise defaults. Their `[roots]` and `[check]` tables are ignored, and they are scanned
anchors-only (§3.3). Root cycles are therefore impossible. Config is parsed defensively (schema,
bounds, spans), but the *roots it declares are trusted*: a config pointing a root at `~` walks
`~`. That is the user's choice, the same as running `rg` there.

---

## 5. CLI (`cli.rs`)

```
anchr check [PATHS...] [--root DIR] [--format human|json] [--strict] [--color auto|always|never]
anchr init  [--agent claude|agents-md|none] [--force] [--dry-run]
anchr completions <shell>                        # clap_complete, static
anchr --version
```

`--format` and `--color` are `ValueEnum`s, never booleans. `--strict` promotes every unverified
finding to an error (§3.5).

`PATHS` filter which files' *references and malformed markers* are reported (indexing still
covers the whole root so anchor resolution is correct; duplicate-anchor and file-level findings
are reported regardless of the filter because they affect the whole root).

`init` is the only writing command in milestone 1. Rules: never overwrite an existing file
without `--force`; `--dry-run` prints what would be written; every path written is printed. It
writes `anchr.toml` and an `AGENTS.md`-compatible instruction block. For `--agent claude` it
merges a `PostToolUse` hook into `.claude/settings.json` via a `serde_json::Value`
read-modify-write that preserves every key it does not own, and refuses (with the exact JSON to
paste) if that file is not valid JSON. Idempotent: running `init` twice is a no-op the second
time. `lsp`, `backrefs`, `rename`, `coverage` are v1.1 subcommands and slot in
as `commands/*.rs` with no structural change.

---

## 6. Crate choices

Versions verified on crates.io 2026-09-04. Reuse-first: the only from-scratch pieces are the
target grammar parser, the region-complement logic, the index, resolution, and grouping.

| Concern | Crate | Version | Why |
|---|---|---|---|
| Markdown | `pulldown-cmark` | 0.13.4 | byte-range `into_offset_iter`; pure Rust; rustdoc/mdBook lineage. **Deviation from `DESIGN.md` (comrak)**: comrak's sourcepos is 1-based line/col and we only need byte exclusion ranges. |
| Parse runtime | `tree-sitter` | 0.27.0 | canonical bindings; accepts grammar ABI 13–15 |
| Grammars | `tree-sitter-rust` 0.24.2, `-typescript` 0.23.2, `-javascript` 0.25.0, `-python` 0.25.0, `-go` 0.25.0 | | each exports `LANGUAGE` + `TAGS_QUERY`; static link, ~5 MB total (estimate) |
| Walk + globs | `ignore` + `globset` | 0.4.33 / 0.4.20 | ripgrep's; parallel walker with per-thread state |
| Marker regex | `regex` | latest 1.x | regular language, linear-time |
| Line/col | `line-index` | 0.1.2 | rust-analyzer's; UTF-16 conversion for LSP built in |
| Paths | `camino` (+ `dunce` on Windows) | 1.2.5 / 1.0.5 | UTF-8 paths end-to-end; no `OsStr` seepage into JSON |
| Config | `toml` + `serde` | 1.1.5 / 1.0.229 | `Spanned<T>`, `de::Error::span()` |
| CLI | `clap` derive + `clap_complete` | 4.6.6 / 4.6.9 | |
| Human output | `annotate-snippets` + `anstream` | 0.12.16 / 1.0.0 | rustc's renderer; multi-file groups; byte spans |
| JSON output | `serde_json` | 1.0.151 | own schema; `serde-sarif` 0.8 later for code scanning |
| Suggestions | `strsim` | 0.11.1 | `osa_distance`, rustc's rule |
| Errors | `thiserror` (core) / `anyhow` (bin) | 2.0.20 / 1.0.104 | |
| Tests | `insta` 1.48.0, `assert_cmd` 2.2.2, `predicates` 3.1.4, `tempfile` 3.27.0, `proptest` 1.11.0 | | |
| LSP (v1.1) | `lsp-server` + `ls-types` | 0.10.0 / 0.0.6 | sync loop, no tokio; `lsp-types` and `tower-lsp` are unmaintained |
| Release | `cargo-dist` (`dist`) | 0.32.0 | per `DISTRIBUTION.md` §3; musl targets |
| Supply chain | `cargo-deny` (`cargo-audit` subsumed) | 0.20.2 | |

Not used, deliberately: `rayon` (walker already parallel), `tree-sitter-tags` (we run the query),
`path-clean` (`RelPath::parse` does its own component walk), `bincode` (unmaintained tombstone),
`miette`/`ariadne`, `tree-sitter-loader` (dynamic loading breaks the static-binary requirement).

---

## 7. Error handling model

- `anchr-core`: every fallible fn returns `Result<T, SpecificError>` with `thiserror` enums per
  module (`ConfigError`, `ScanError`, `ContainerError`). No `unwrap`/`expect` in library code
  (`clippy::unwrap_used`, `clippy::expect_used` = deny at workspace level; tests are exempt via
  `#[cfg_attr(test, allow(...))]`).
- Two channels, kept apart on purpose: **tool failures** propagate as `Err` (exit 2);
  **findings about the user's files** — broken refs, malformed markers, unreadable/too-large
  files — are data inside `Report`. A single unreadable file must never abort a check; it
  becomes an *unverified* diagnostic so it is visible but non-blocking.
- Binary: `anyhow` at the top, `main` maps `Ok(report)` → exit code from `report`, `Err` → stderr
  + exit 2.

---

## 8. Concurrency and state

- Immutable inputs (`RootSet`, `Config`, registry) shared by `&`; per-file work is pure
  (`&str → FileScan`) and runs on the walker's threads, each owning its `FileAnalyzer`; results
  flow over an `mpsc` channel to a single-threaded fold into `Index`. Resolution and rendering
  are single-threaded, so their caches are plain `HashMap`s owned by the `Resolver`. No global
  mutable state; the marker `Regex` is a `LazyLock` static and the `LanguageRegistry` is built
  once per run and shared by `&`.

---

## 9. Testing

- **Unit** (in each module): `parse_target` table tests incl. every rejection reason
  (qualified symbol, reserved chars in path segments, `.`/`..`, trailing-slash expectation,
  root prefix on each kind); `AnchorId` charset; lexer with `proptest` (round-trip: any generated
  valid marker embedded in random text is found with the right span; any text without
  `@anchor[`/`@ref[` yields none) plus a regression corpus for CRLF files and `\r` in a body,
  multi-byte text before a marker (offset and line/col both), and a non-ASCII preceding char;
  markdown regions with `insta` snapshots on fixtures covering fenced/indented code, inline code,
  headings, HTML comments, reference-style links, marker split by `[`, backslash-escaped marker
  not lexed, fence inside a blockquote and inside a list item, tilde fence, longer closing fence,
  unclosed fence at EOF, indented code inside a nested list (the complement approach's only
  failure mode is an imprecise code range letting an example through, so these are the tests
  that matter); per-language source fixtures asserting comment extraction and `SymbolTable`
  contents against a **per-kind declaration list written first** (function, struct/class,
  interface, enum, type alias, method, const/arrow-function export, module), plus one fixture
  per language with deliberately broken syntax around a declaration asserting
  `Unverified::ParseErrors` rather than `SymbolMissing`; `LanguageRegistry::new()` succeeds.
- **Integration** (`crates/context-anchors/tests/`): fixture mini-repos under
  `tests/fixtures/<case>/` with an expected `report.json` snapshot via `insta`; `assert_cmd`
  for exit codes 0/1/2, `--format json` schema stability, `--strict`, absent external root,
  duplicate anchors, cross-root refs, undeclared root ⇒ error vs absent root ⇒ unverified,
  external root's refs not reported, a gitignored `.md` matching `include` is *not* scanned,
  symlink not followed, oversized file reported as unverified with the anchors-unindexed wording,
  `@ref[src/Foo.ts]` against `foo.ts` is `PathMissing` on every platform (exact-name lookup),
  `PATHS` filtering semantics, a human-output `insta` snapshot with `--color never`, a
  bad-config exit-2 snapshot showing the caret rendering, and `init` idempotency / `--force` /
  `--dry-run` / settings-merge preserving foreign keys.
- **Dogfood**: `anchr.toml` at repo root; CI runs `anchr check` on `DESIGN.md` /
  `DISTRIBUTION.md` (their inline `@ref[...]` examples live in fences, which is itself a test of
  fence exclusion).
- **Security gates in CI**: `cargo deny check`, `cargo audit`, `cargo clippy -D warnings`,
  `cargo test`, plus `cargo build --release` size check for the binary (grammar bundle budget).

---

## 10. Security posture (from the corgea checklist)

Threat model: every walked file, every config file, and every marker body is untrusted input. A
hostile repository must not be able to crash the checker, read outside its roots, or exhaust
memory. Concrete commitments, mapped to the checklist items they satisfy:

**Workspace lints and profile** (items 1, 2, 7, 9, 10, 11, 15, 24) — declared once in the
root `Cargo.toml`, every crate opts in with `lints.workspace = true`:

```toml
[workspace.package]
edition = "2024"
rust-version = "1.85"          # tested in CI alongside stable

[workspace.lints.rust]
unsafe_code = "forbid"         # both crates; the tree-sitter crate owns the FFI, we never touch it
                               # no missing_docs: it would force a doc comment on every pub item,
                               # against the "default to no comment" rule

[workspace.lints.clippy]
unwrap_used = "deny"           # allowed in #[cfg(test)] modules only
expect_used = "deny"
cast_possible_truncation = "deny"   # forces try_from for usize → u32 in JSON/LSP positions
undocumented_unsafe_blocks = "deny" # moot under forbid, kept so a future exception is visible
redundant_clone = "warn"
needless_pass_by_value = "warn"

[profile.release]
lto = "fat"
codegen-units = 1
opt-level = 3
strip = "symbols"
overflow-checks = true         # span arithmetic on file-derived offsets must never wrap
panic = "unwind"
```

`#[must_use]` on `Report`, `Resolution`, and every `parse` constructor. **No** `#[non_exhaustive]`
on core enums: the binary crate matches `DiagnosticKind`, `Unresolved`, and `Unverified`
exhaustively in both renderers, and `non_exhaustive` (which applies across crates) would force
`_ =>` arms there and defeat the "add a variant, every match fails to compile" property we want.
It goes only on the JSON DTO enums that external consumers deserialize.

**Panics policy** (items 6, 7) — a panic is a bug, never control flow. In the CLI there is no
`catch_unwind` around per-file work: all FFI sits behind the `tree-sitter` crate's safe API, so
there is no panic-into-C risk of ours, and swallowing panics would hide defects that fuzzing is
supposed to surface. A panic on a walker thread is expected to propagate when the walk joins its
threads; step 1 verifies this with a test (the `ignore` crate's behaviour here is plausible but
unconfirmed) and adds an explicit join-and-resume if it does not. The LSP (step 9) is the one
exception: a panic on one document must not kill the editor session, so the request loop wraps
each request in `catch_unwind`, logs it, and answers with an LSP error. Everything that can
legitimately go wrong with a *file* is a `Result` or an unverified finding.

**Path traversal** (items 4, 20) — `RelPath::parse` walks `Utf8Path::components()` and rejects
`RootDir`, `Prefix`, `ParentDir`, and backslashes before any join ever happens (purely lexical,
no filesystem access, so it also works for paths that do not exist yet). For *reads* (symbol
resolution parses the target file), the joined path is additionally `canonicalize`d and checked
with `starts_with(root_canonical)` (`dunce::simplified` on the root on Windows); a symlink that
escapes the root yields `Unresolved::PathEscapesRoot` rather than a read. Existence-only path refs use
`symlink_metadata` and never follow. The walker runs with `follow_links(false)`. Functions take
`&Utf8Path`, not owned paths. A `proptest` asserts `RelPath::parse(s).map(|p| root.join(p))`
never escapes `root` for arbitrary `s`.

**Input bounds** (items 14, 16, 19) — `AnchorId`, `RootName`, `SymbolName`, and `RelPath` are
allowlist-parsed newtypes with private fields and length caps (ID ≤ 256 bytes, segment ≤ 64,
path ≤ 1024). File size is checked from metadata before reading (`max-file-bytes`, default
2 MiB, validated ≤ `u32::MAX`); oversized files are unverified findings. The marker regex is
`regex` (linear time). Tree-sitter parsing runs under a progress-callback budget
(`ParseTimeout`), and `SymbolTable` construction caps at 100k declarations per file, discarding
the table and reporting `SymbolTableTruncated` rather than returning a partial one. No
`with_capacity(n)` where `n` derives from file content. Config: `deny_unknown_fields`, enums for
every fixed-choice field (`UnverifiedPolicy { Report, Error }`, `PathExpectation`, `ScanMode`,
never a bool), collection caps validated right after deserialization, `~` expansion only on
`[roots]` values.

**Integer handling** (item 15) — all offsets are `usize` internally; conversion to `u32` for JSON
and LSP goes through `u32::try_from` and a `PositionOverflow` error. `&str` slicing at
file-derived offsets uses `get(..)`, never `[..]`, because a tree-sitter or pulldown offset that
lands mid-codepoint would otherwise panic.

**No shell, bounded output** (items 18, 21) — no `std::process::Command` anywhere in milestone 1:
git toplevel detection walks up for a `.git` entry instead of invoking `git`. JSON output
contains only paths, spans, marker bodies, IDs, and suggestions, never file content. The human
renderer does print source lines, but only the lines containing a marker site in a file the
scan already selected (a `.env` is not a scanned container), and only for the first site of
each diagnostic.

**Supply chain** (item 3) — grammar crates compile C in `build.rs`, so each is vetted (repo,
maintainer, download count) before adding; `deny.toml` per the site's template with
`unknown-git = "deny"`, `wildcards = "deny"`, license allowlist MIT/Apache-2.0/BSD-3/ISC/
Unicode-3.0; `Cargo.lock` committed; CI builds with `--locked`; `cargo audit` + `cargo deny check`
on every PR; `cargo geiger` run once to record the unsafe budget the tree-sitter stack brings in.

**Gaps the checklist does not cover, handled anyway** — symlink loops: the walker never follows
links, and the only reads outside the walk are canonicalized symbol-resolution targets. Open-file
pressure: the walker's thread count bounds concurrent reads and each file is read whole and
dropped before the next. Char boundaries: covered under integer handling above. JSON output:
`serde_json` escapes control characters; invalid UTF-8 cannot reach it because non-UTF-8 files
are rejected at read time.

**Testing** (item 23) — `cargo fuzz` targets for `parse_target`, `lex`, `markdown::text_regions`,
`source::text_regions` (per language), and `Config::from_str`: must never panic or hang.
`proptest` on the lexer and `RelPath`. CI: `fmt --check`, `clippy -D warnings`, `test --locked`,
`audit`, `deny check`, MSRV job.

---

## 11. Build order

The version labels in `DESIGN.md` §10 are milestones, not releases: everything below is built in
succession before the first public release, so the core is designed for the final shape from
step 1 (the `Index` update/remove API, the DTO-decoupled JSON schema, and the `LanguageSpec`
table all exist because steps 9–11 need them).

**Milestone 1 — the guarantee (`anchr check`)**
1. Workspace scaffold, lints, `deny.toml`, CI skeleton, `cargo-dist` config. Two spike tests
   that pin third-party behaviour the design depends on: a walker-thread panic propagates to
   the caller, and a gitignored file is not yielded even when it matches an include glob.
2. `span`, `marker` (types + `parse_target` + lexer) — pure, fully unit-tested first.
3. `text/markdown`, `text/plaintext`, `text/language` + `text/source` (one language first: Rust,
   to dogfood; then TS/TSX/JS/Py/Go).
4. `config`, `root`, `scan`, `index`.
5. `resolve/*`, `suggest`, `diagnostic`, `check::run_check`.
6. Binary: `cli`, `check` command, human + JSON render, exit codes; integration fixtures.
7. `init`; dogfood `anchr.toml`; fuzz targets.
8. Release pipeline: cargo-dist (shell/powershell installers, musl targets), plus the npm
   platform-package script (`@context-anchors/<platform>` + thin `anchr` shim generated from
   `dist-manifest.json`), dry-run against a pre-release tag.

**Milestone 2 — the multiplier**
9. `anchr lsp` (`lsp-server` + `ls-types`, stdio): diagnostics on open/change, go-to-definition
   for `#id` (anchor `Site`) and `file#Symbol` (`SymbolTable` declaration spans),
   find-references via `Index::backrefs`, rename of an anchor ID via the recorded `id_span`s,
   document symbols. Reuses `check::run_check` machinery per root and `Index::update_file` per
   edited document; `catch_unwind` per request (§10).
10. `anchr backrefs`, `anchr rename` (rename is the only mutating command besides `init`; it
    rewrites exactly the `id_span` bytes of every anchor and ref carrying the old ID and prints
    every file touched).
11. `anchr coverage` / `anchr annotate`: heuristic scanner over the same text regions, reporting
    reference-shaped strings that are not annotated; never errors, never writes on its own.

**Milestone 3 — review ledger and ecosystem** (design pass required before code)
12. `anchr review` / `anchr accept` signature ledger; exported vs. internal anchors; MCP adapter
    if warranted. These need their own code-level design; out of scope for this plan.

Each numbered step is one PR-sized unit and independently testable.

---

## 12. Decisions that deviate from DESIGN.md / DISTRIBUTION.md

All approved with the design; each is independent, so any one can be reversed without
disturbing the rest.

1. **pulldown-cmark instead of comrak** — byte ranges natively; see §6.
2. **No persistent index in v1** — see §3.4; survey independently reached the same conclusion.
3. **`root:` prefix allowed on all target kinds**, not only `#id` — see §2.
4. **Human renderer is annotate-snippets**, so the report is rustc-shaped (one snippet + site
   list) rather than the pure `path:line` list sketched in `DESIGN.md` §6.
5. **npm channel conflict.** `DISTRIBUTION.md` §4 requires the esbuild-style
   `optionalDependencies` platform-package layout and explicitly rejects a postinstall download
   shim. cargo-dist 0.32's npm installer *is* a download shim (one package, fetches the archive
   from GitHub Releases at install time). Options: (a) ship the shim in v1 and revisit; (b) write
   a small release-workflow script that builds the platform packages from cargo-dist's
   `dist-manifest.json` (esbuild/biome pattern; roughly a day of work); (c) defer npm to v1.1 and
   ship curl + GitHub Releases only. **Decision: (b).** The user wants the final shape, and all
   milestones are built before anything is released, so there is no interim to optimize for.
   `DISTRIBUTION.md` §4 stands as written.
6. **LSP stack for v1.1**: `lsp-server` + `ls-types`, synchronous, negotiating
   `positionEncoding: utf-8` with UTF-16 fallback via `line-index`. Not tower-lsp (dead) or
   tokio-based servers (unneeded for a re-parse-whole-document server).

## 13. Research appendix

The research this design rests on lives in `docs/research/`:

- `rust-security-best-practices-digest.md` — corgea checklist (25 items) with a mapping onto a
  filesystem-walking, tree-sitter, TOML-config, JSON-emitting CLI. §10 is derived from it.
- `crate-survey.md` — 15 areas, versions verified on crates.io 2026-09-04, API sketches, gotchas.
  §6 is derived from it.
- `code-design-review.md` — 25 findings against the draft of this document, all incorporated.
  The three blockers were: include globs as `ignore` overrides bypassing `.gitignore`, false
  `SymbolMissing` on error-bearing parse trees, and case-insensitive path resolution making
  `check` platform-dependent.

Survey caveats to verify during implementation: compiled grammar sizes are estimates; `ignore`
override-vs-gitignore precedence is pinned by a spike test in build step 1 rather than assumed.
