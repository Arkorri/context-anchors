# context-anchors — code-level design

**Status:** approved 2026-09-04
<!-- refs -->
@ref[crates/anchr-core/src/marker/target/target.rs#parse_target as parse_target]
@ref[crates/anchr-core/src/text/text.rs#FileAnalyzer as FileAnalyzer]
@ref[crates/anchr-core/src/index/index.rs#Index as Index]
@ref[crates/anchr-core/src/index/index.rs#Site as Site]
@ref[crates/anchr-core/src/marker/path/path.rs#RelPath as RelPath]
@ref[crates/anchr-core/src/resolve/resolve.rs#Resolver as Resolver]
@ref[crates/anchr-core/src/diagnostic/diagnostic.rs#Report as Report]
@ref[crates/anchr-core/src/diagnostic/diagnostic.rs#DiagnosticKind as DiagnosticKind]
@ref[crates/anchr-core/src/root/root.rs#FilePath as FilePath]
@ref[crates/anchr-core/src/text/source/source.rs#SymbolTable as SymbolTable]
@ref[#cli/check as Check]
@ref[#cli/coverage as Coverage]
@ref[#cli/backrefs as Backrefs]
@ref[#cli/rename as Rename]
@ref[#cli/init as Init]
@noref[foo.ts, report.json, docs/a.md, .claude/, .claude/worktrees/, research/, marker/lex/lex.rs, marker/lex/lex_tests.rs, _tests.rs, _test.rs, tests/]

**Companion to:** @ref[DESIGN.md] (what the tool is) and @ref[DISTRIBUTION.md] (how it ships); this
document covers how the code is shaped. Where it deviates from those two, §12 says so.

## Context

@ref[DESIGN.md] and @ref[DISTRIBUTION.md] settle *what* `anchr` is: opt-in `@anchor[id]` /
`@ref[target]` markers in prose and code comments, batch-validated by `anchr check`,
grouped-by-cause diagnostics with an explicit *unverified* class, Rust single binary, LSP later. The
repo has no code yet.

This document is the code-level design that turns that into a Rust workspace: crate layout, module
map, core types, the per-stage algorithms, the crate choices (reuse first), error/diagnostic model,
security posture, CLI surface, config schema, and test strategy. Open questions from
@ref[#design/open-questions] are answered inline where the code forces a decision. Question 4
(premise validation) is skipped per instruction.

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

Package names: `anchr-core` (lib), `context-anchors` (bin → `anchr`). Matches @ref[#dist/name].

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
pub struct RelPath(Utf8PathBuf);        // root-relative, normalized, from a bare or a ./|../ spelling
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

`Option<RootName>` on every target variant: the `root:` prefix is allowed on all three target kinds,
not only on `#id`. @ref[#design/grammar] only shows `root:#id`, but the grammar is `[root:]target`
and making paths/symbols cross-root capable costs nothing and keeps one rule (§12 item 3).

### Target grammar (@[parse_target])

```
ref         := target [ws "as" ws alias]          alias clause declares a file-local name
use         := "@[" alias "]"                     a use of a declared alias
alias       := [A-Za-z_] [A-Za-z0-9_]*            max 64 bytes
noref       := "@noref[" entry ("," ws* entry)* "]"   strings that are not references in this file
entry       := glob, 1..=256 bytes, no whitespace, neither @ nor `   (matched against tokens, never a target)
target      := [root ":"] body
body        := "#" anchor_id                      -> Anchor
             | rel_path "#" symbol_name           -> Symbol
             | rel_path ["/"]                     -> Path   (trailing "/" ⇒ PathExpectation::Directory)
root        := [A-Za-z0-9_-]+
anchor_id   := segment ("/" segment)*
segment     := [A-Za-z0-9_] [A-Za-z0-9_.-]*
rel_path    := path_seg ("/" path_seg)*                  root-relative
             | "./" path_seg ("/" path_seg)*             relative to the writing file's directory
             | ("../")+ path_seg ("/" path_seg)*         the same, climbing; never above the root
path_seg    := one or more printable non-whitespace chars excluding  [ ] # : \  and control chars;
               "." and ".." rejected except as the leading run above; leading "/" rejected
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

`RelPath::parse` is an allowlist, like the other newtypes: it walks segments, rejects `.` and
`..`, checks each segment's charset, and never touches the filesystem. `RelPath::anchored` is
the second constructor: the lexer passes the file being lexed, a `./` or `../` spelling is
normalized lexically against that file's directory (@ref[crates/anchr-core/src/tree/tree.rs#normalize]),
climbing above the root is `PathError::EscapesRoot`, reported at the marker's span through the
ordinary malformed-target path, and a `root:` prefix on a relative spelling is a parse error
because "relative to this file" has no meaning in another root. Either way the stored path is
root-relative, so the resolver, the index, `backrefs`, and the LSP never see the written form;
`@ref[./x.md]` in `docs/a.md` and `@ref[docs/x.md]` in the README are one target. @[parse_target]
records `PathExpectation::Directory` *before* normalization, because `Utf8PathBuf` drops
trailing separators. Resolution joins onto `Root.dir`; the normalized form is what guarantees
the join cannot escape the root (§10). A target typed on the command line (`backrefs`) has no
file, so a relative spelling there is `TargetError::RelativeNeedsFile`.

@[parse_target] also returns the byte span of the ID portion for `#id` / `root:#id` targets so
@[Rename] (step 10) rewrites exactly the ID bytes and nothing else.

---

## 3. Pipeline stages

```
RootSet (config) ─► scan each present root ─► FileScan{markers, malformed} ─► Index
                                                                              │
                     Report ◄── group ◄── Diagnostics ◄── Resolver ◄──────────┘
```

Single entrypoint: `check::run_check(root_set, options) -> Result<Report, CheckError>`.
@ref[crates/anchr-core/src/check/check.rs#CheckError] is tool failure (bad config, unreadable root) →
exit 2. Broken references are *data* in @[Report], never
`Err`.

### 3.1 Text regions (which bytes may contain markers)

`text::TextRegions` is a sorted `Vec<(ByteSpan, RegionKind)>` of *included* ranges. The lexer only
ever runs over included ranges, so the container is the only thing that decides "prose vs example".
@ref[crates/anchr-core/src/text/text.rs#Container] is a pure selector enum (closed set, no trait
object); the work happens in a per-thread @[FileAnalyzer],
because tree-sitter's `Parser` is `!Sync` and must not be constructed per file:

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
`Comment`. Same-line backtick spans inside a comment are carved out: doc comments are
markdown by convention, so `` `@ref[x]` `` in one is an example, exactly as in prose (this is
what lets this repository's own doc comments describe the grammar and still pass `anchr
check`). Using the same query mechanism as symbol lookup (§3.5) means one API for both jobs.
Parse errors from tree-sitter do not fail the file: comments still extract from an
error-tolerant tree. A file whose extension has no registered language is not a source
container at all (see registry below). `Parser` is `!Sync`, so one lives per walker thread
(§3.3); `Query` objects are compiled once per language and shared.

**Plaintext** — one region `0..len`, kind `Whole`. Answers @ref[#design/open-questions] Q3 for v1: a
plaintext container cannot carry documentation about the marker syntax, and that is acceptable; the
escape hatch is to write such docs in markdown. Note this in the user docs.

**LanguageRegistry** (@ref[crates/anchr-core/src/text/language/language.rs#LanguageRegistry]): table `ext →
LanguageSpec { name, language: Language, comment_query: Query, declaration_query: Option<Query> }`
for the core bundle: TypeScript and TSX (two parsers from one crate), JavaScript, Python, Rust, Go;
markdown is handled by pulldown. Constructed by `LanguageRegistry::new() -> Result<Self,
RegistryError>` once in @ref[crates/anchr-core/src/check/check.rs#run_check] and passed by `&`, not in a
`LazyLock`: `Query::new` returns `Result` (ABI or query-syntax drift is a real runtime failure) and
a static initializer would have to `expect`. A unit test asserts construction succeeds so drift
fails CI.

@ref[crates/anchr-core/src/text/language/language.rs#declaration_query] is the grammar crate's `TAGS_QUERY`
(tree-sitter's `tags.scm`, verified exported by all five) *concatenated with* a per-language
`supplementary_declarations` string, because `tags.scm` coverage is uneven — TypeScript's is short
and is not expected to capture `type_alias_declaration`, `enum_declaration`, or `export const f = ()
=> {}`, the dominant declaration form in TS. The per-kind fixture list in §9 is written before the
supplementary queries so gaps surface as failing tests, not as false `SymbolMissing` errors. Grammar
crates depend only on `tree-sitter-language`, so the runtime version is pinned independently
(`LANGUAGE.into()`). Adding a language = one table row + one dependency. Known caveat:
`tree-sitter-typescript` is the stalest grammar (late 2024); newer TS syntax parses with errors,
which §3.5 turns into an *unverified* outcome rather than a false error.

**Container selection**: by extension, from `Config.containers` merged over defaults.
Extension not in any list → file is not scanned at all (opt-in principle applies to files as
well as markers).

### 3.2 Marker lexing (@ref[crates/anchr-core/src/marker/lex/lex.rs])

The marker language is regular. One compiled `regex::Regex` in a `LazyLock`:

```
@(anchor|ref|noref|)\[(?:([^\[\]\n]*)\]|)
```

An empty kind is an alias use, `@[X]`; brackets keep it the same shape as every other marker, so
`@param` in a comment is never one. `@noref` bodies are comma-separated lists parsed by
@ref[crates/anchr-core/src/marker/noref/noref.rs#parse_noref_body]; each entry keeps its own span so
@[Coverage] can point at an unused one. The empty body alternative catches an opener with no closer
on the same line → `Unclosed`. A post-filter
rejects matches whose preceding *character* (`source[..start].chars().next_back()`, not the
preceding byte, so `é@ref[x]` and `e@ref[x]` behave the same) is alphanumeric or `_`, so
`foo@ref[...]` inside an email-like token is not a marker. Because the lexer runs over the source
slice and not the rendered text, a markdown backslash escape `@ref\[x\]` is never lexed; this is the
documented way to show a literal marker in prose outside a code fence. For each region, run the
regex on `&source[span]`, offset match positions by `span.start`. Body goes to `AnchorId::parse` or
@[parse_target]; failures become @ref[crates/anchr-core/src/marker/marker.rs#MalformedMarker]. Multiple
markers per line are naturally supported.

Output per file: `FileScan { path, markers: Vec<Marker>, malformed: Vec<MalformedMarker>, line_index: LineIndex }`.

### 3.3 Scan (@ref[crates/anchr-core/src/scan/scan.rs])
<!-- @anchor[code/scan] -->

For a @ref[crates/anchr-core/src/root/root.rs#Root]: `ignore::WalkBuilder::new(root.dir)` with
`.hidden(false)`, `.git_ignore(true)`, `.git_global(true)`, `.git_exclude(true)`,
`.require_git(true)`, `.ignore(false)`, `.follow_links(false)`, and one `filter_entry` that
prunes any entry named `.git` at any depth and any entry `config.ignore.paths` matches.
`require_git(true)` makes `.gitignore` behave exactly as git does: it applies only inside a
repository, and parent `.gitignore` files stop at the nearest `.git`. The earlier
`require_git(false)` read every `.gitignore` up to the filesystem root, so a copy of this
repository under `~/.claude` lost @ref[docs/research/] to an unanchored `research/` line in
`~/.claude/.gitignore` (§12a item 20). ripgrep `.ignore` files are off: the knobs are
`.gitignore` and `[ignore] paths`, nothing else. `paths` is a
@ref[crates/anchr-core/src/config/config.rs#IgnoreConfig] `Gitignore` built from the config lines and
consulted with root-relative paths; because the filter runs after the walker's own gitignore
pass, a `!` line there can never resurrect a gitignored file, which is what "layered on
`.gitignore`" promises. Hidden files are walked because `.claude/skills/**/SKILL.md` and
`.github/workflows/*.yml` are exactly the documentation this tool checks; `.gitignore` and
`[ignore] paths` are the knobs for dotdirs that should not be, the same way they are for
everything else. `.git` is pruned rather than merely unscanned so its object and hook extensions
never reach the coverage extension table (§12a item 15). A repository that keeps worktrees under
`.claude/worktrees/` must list them, in `.git/info/exclude` or `[ignore] paths`, or every anchor
id appears twice. `include` is *not* a walker override: `ignore` consults overrides before
ignore files and returns on any override match, so a whitelist glob would silently un-ignore
gitignored files (`CHANGELOG.md`, `*.generated.ts`). Instead `include` is compiled to a
`globset::GlobSet` (`literal_separator(true)`) and applied as a post-filter in the visitor, after
the walker has already applied `.gitignore`. A two-line integration test pins this: a gitignored
`.md` file must not be scanned.

`build_parallel()` gives a thread per core with a per-thread visitor. Each visitor owns a
@[FileAnalyzer] (§3.1) and a clone of an `mpsc::Sender<ScanOutcome>`. Every file and symlink
the walk yields is first sent as a tree entry (a symlink with its `read_link` text, never
followed), before any include or container gating, so @ref[Cargo.lock] and `image.png` are in
the tree though never lexed. Then, per file with a container: `metadata` size check (>
`config.scan.max_file_bytes`, default 2 MiB → `SkippedFile::TooLarge`), `std::fs::read` + UTF-8
validation (non-UTF-8 → `SkippedFile::NotUtf8`), `analyzer.scan(path, source)`, send. The main
thread drains the channel into `Vec<FileScan>` + `Vec<SkippedFile>` + a
@ref[crates/anchr-core/src/tree/tree.rs#FileTree]. No `rayon`: the walker already provides the
parallelism, and channel-to-single-reducer avoids a shared `Mutex<Vec>`. Paths convert to
`Utf8PathBuf` at this boundary; a non-UTF-8 file name is a `WalkProblem`.

**The scan is the authority on what exists.** It is a pure function of the filesystem and the
ignore rules, rerun on every invocation, and its tree is the only thing path resolution consults
(§3.5). A file that git ignores or that `[ignore] paths` lists is on this machine and not in the
repository, so it does not exist as a target and a clean checkout agrees with the local run.

**External roots are scanned in `ScanMode::AnchorsOnly`.** Their refs and malformed markers are
discarded: the user is checking *their* root, and an external root's own cross-root refs could
not resolve anyway (its `[roots]` table is not loaded). Only its anchors are indexed. A
duplicate anchor ID inside an external root is reported as *unverified* (`ExternalDuplicate`),
not as an error, because it cannot be fixed from here.

A skipped file (too large, not UTF-8) may contain anchors that are therefore unindexed, which
can surface elsewhere as `AnchorMissing`. The `FileTooLarge` / `FileNotUtf8` diagnostics say so
explicitly and name `max-file-bytes` as the knob, so the cause is visible in the same report.

### 3.4 @[Index] (@ref[crates/anchr-core/src/index/index.rs])

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
@ref[crates/anchr-core/src/index/index.rs#update_file] and
@ref[crates/anchr-core/src/index/index.rs#remove_file], so the two can never drift.

**No persistent cache in v1.** @ref[#design/architecture] says "incremental, gitignored"; the scan
is embarrassingly parallel and tree-sitter is fast enough that a full rebuild on a mid-size repo is
well under a second, and an on-disk cache adds an invalidation surface with no correctness benefit
("never authoritative"). The @[Index] API above is already
incremental for the LSP's in-memory needs. If measurement later says otherwise, persistence goes in
a cache dir (`directories` crate) keyed by root path, *not* inside the repo, so nothing needs
gitignoring (§12 item 2).

### 3.5 Resolution (@ref[crates/anchr-core/src/resolve/])

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

@[Resolver] holds `&RootSet`, `&HashMap<RootName, Index>` (each index carrying its root's scan
tree), its own @[FileAnalyzer], and a per-run `symbol_cache: HashMap<(RootName, RelPath),
SymbolTable>` so a file referenced by 40 refs is parsed once. Resolution runs single-threaded
after the fold (it is cheap relative to the scan), so these caches need no synchronization.

**Root selection happens once, before dispatch on target kind**, so all three variants share one
rule: `None` ⇒ current root; a name not in @ref[crates/anchr-core/src/root/root.rs#RootSet] ⇒
`Unresolved::RootUndeclared` (a typo, with a suggestion from the declared names);
`RootStatus::Absent` ⇒ `Unverified::RootAbsent`; otherwise resolution proceeds against that root's
dir and index.

- **Path**: a lookup in the root's scan tree (§3.3), never on the disk. A file exists when the
  walk enumerated it; a directory exists when the walk enumerated something beneath it, so an
  empty directory or one whose every file is ignored does not. Gitignored paths and paths listed
  in `[ignore] paths` are therefore missing, and a missing path that is nevertheless on disk
  gets a note naming the cause: the `paths` line that pruned it, or `.gitignore`. Keys are
  the bytes the walker reported, so on a case-insensitive filesystem (macOS default)
  `@ref[src/Foo.ts]` still fails against `foo.ts` exactly as in Linux CI (invariant 5,
  @ref[#design/invariants]). A symlink entry is redirected lexically: substitute its link text
  for the longest recorded prefix, normalize, look up again, at most `MAX_SYMLINK_HOPS` times;
  a link whose target leaves the root is the one case that falls back to `fs::metadata`, for
  existence only, because the tree has no ignore information out there and
  `docs -> ../shared-docs` is a promised shape. A trailing `/` in the target additionally requires
  a directory. Directory refs stay (@ref[#design/open-questions] Q1: free, so keep).
- **Symbol**: path must exist (else `PathMissing`); the joined path is canonicalized and must
  `starts_with` the canonical root (else `PathEscapesRoot`, since this is a read); registry lookup
  by extension (none ⇒ `NoGrammar`; no declaration query ⇒ `NoSymbolQuery`); parse with
  `parse_with_options` and a progress callback that aborts past a wall-clock budget (⇒
  `Unverified::ParseTimeout`); run
  @ref[crates/anchr-core/src/text/language/language.rs#declaration_query] with `QueryCursor::matches` (a
  `StreamingIterator` in tree-sitter 0.27, so `while let Some(m) = matches.next()`); per match, keep
  it only if it has a capture whose name starts with `definition.` (a per-match filter; there is no
  clean pattern-to-capture API for `disable_pattern`), and record the `@name` capture's text and
  `byte_range()` into `SymbolTable { declarations: HashMap<String, Vec<ByteSpan>>, has_parse_errors:
  bool }` — spans are what go-to-definition (step 9) needs. If the declaration cap (100k) is hit the
  table is discarded and the outcome is `Unverified::SymbolTableTruncated`, never a truncated table
  that yields false misses. Outcome: name found ⇒ Resolved; not found and `root_node().has_error()`
  ⇒ `Unverified::ParseErrors { path, language }` with a hint about grammar staleness (a declaration
  inside an ERROR node is invisible to the query, and reporting that as missing would be a false
  positive); not found and clean tree ⇒ `SymbolMissing`. Guarantee is exactly
  @ref[#design/architecture]'s: *a declaration with this name exists in this file*, any nesting
  depth. Running the query ourselves (~40 lines) rather than pulling `tree-sitter-tags` keeps one
  API for comments and declarations.
- **Anchor**: `index.anchor_sites(id).is_empty()` in the selected root ⇒ `AnchorMissing`.
  Duplicated IDs still resolve (the duplicate is its own error at the anchor sites, or an
  `ExternalDuplicate` unverified finding if the root is external).

Answers @ref[#design/open-questions] Q2, more broadly than asked: every unverified outcome (absent
root included) is non-blocking by default; `--strict`, or `check.unverified = "error"` in config,
promotes *all* unverified findings to errors. CI users read `--strict` as "fail on anything you
could not check", so the flag means exactly that rather than a single-cause toggle.

### 3.6 Diagnostics and grouping (@ref[crates/anchr-core/src/diagnostic/diagnostic.rs])

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

Grouping key is the @[DiagnosticKind] value itself (derive
`Eq + Hash`; it carries the *cause* and never a location): one `AnchorMissing{auth/flow}` with
twelve sites, not twelve diagnostics; all `Unclosed` `@ref[` markers in one group. File-level
findings use `Locations::Files`. Sites sorted by (path, line). Errors sorted before unverified, then
by location count desc. `Report::has_errors()` drives exit code 1.

Suggestions (@ref[crates/anchr-core/src/suggest/suggest.rs#suggest]): rustc's rule, which fits short
identifiers better than Jaro. Over the candidate set (all anchor IDs in the target root; all symbol
names in the target file; the names beneath the missing component's parent in the scan tree,
so an ignored sibling is never suggested): first a
case-insensitive exact match, then `strsim::osa_distance` (restricted Damerau-Levenshtein) accepted
when `distance ≤ max(len, 3) / 3`, then for hierarchical IDs the same on the last `/` segment. At
most one suggestion, the lowest distance; never mutates.

### 3.7 Rendering (binary crate)

- **Human** (@ref[crates/context-anchors/src/render/human/human.rs]): `annotate-snippets` 0.12, which is
  rustc's own renderer and is built around exactly the cause→N-sites shape: one titled `Group`
  holding several `Snippet`s, each with its own path and byte-span annotations. Output goes through
  `anstream` for TTY detection and `NO_COLOR`. Per diagnostic: title = cause (`unknown id
  \`auth/flow\``), the first site rendered as a source snippet with the marker span underlined, the
  remaining sites as a compact `path:line:col` list (twelve full snippets would bury the report),
  and the suggestion as a `help:` footer. Source text is not retained in the @[Index]; the renderer
  re-reads the one file per diagnostic it renders as a snippet and degrades to the `path:line:col`
  form if that read fails (the file changed or vanished between scan and render). Line/col for every
  site comes from `LocatedSite`, so the list form never needs the source. Multi-file gutter
  alignment is more work than it looks, which is why this is borrowed rather than written; miette
  wants to own error types and ariadne defaults to char offsets, so neither fits.
- **JSON**: `serde` on a dedicated @ref[crates/context-anchors/src/render/json/json.rs#JsonReport]
  DTO (not the core types, so the wire format is decoupled), with `"schema": 1`.
  Locations carry path, 1-based line/col, byte span, and region kind. Unverified diagnostics include
  a `hint` string that names the fix ("install the `full` build or add a grammar for `.ex`" per
  @ref[#dist/implementation-decisions]).

Exit codes: 0 clean (unverified may be present), 1 errors, 2 tool failure.

---

## 4. Config (@ref[crates/anchr-core/src/config/config.rs#Config])

@ref[anchr.toml], discovered with `cwd.ancestors().find(|d| d.join("anchr.toml").is_file())` (or
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
max-file-bytes = 2097152    # validated ≤ u32::MAX (line-index offsets are u32)

[ignore]
paths = []                  # gitignore syntax, layered on .gitignore; not scanned, not a target
tokens = []                 # globs against coverage tokens; never proposed as references

[containers]
markdown  = ["md", "markdown"]
plaintext = ["txt"]
# source containers come from the language registry; not user-extensible in v1

[check]
unverified = "report"       # or "error" (same as --strict)
```

The current root needs a @ref[crates/anchr-core/src/root/root.rs#RootName] for every
@[Site]; `[root] name` provides it, defaulting to the directory's
basename (validated; an invalid basename is a config error that names the fix).

External roots load *their own* @ref[anchr.toml] for `[scan]`, `[containers]`, and `[ignore] paths`
if one exists; otherwise defaults. Their `[roots]`, `[check]`, and `[ignore] tokens` are ignored,
and they are scanned anchors-only (§3.3). Root cycles are therefore impossible. Config is parsed defensively (schema,
bounds, spans), but the *roots it declares are trusted*: a config pointing a root at `~` walks `~`.
That is the user's choice, the same as running `rg` there.

---

## 5. CLI (@ref[crates/context-anchors/src/cli/cli.rs])

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

In milestone 1, @[Init] (@ref[crates/context-anchors/src/commands/init/init.rs]) is the only writing
command. Rules: never overwrite an existing file without `--force`; `--dry-run` prints what would be
written; every path written is printed. It writes @ref[anchr.toml] and an `AGENTS.md`-compatible
instruction block. For `--agent claude` it merges a `PostToolUse` hook into @ref[.claude/settings.json]
via a `serde_json::Value` read-modify-write that preserves every key it does not own, and refuses
(with the exact JSON to paste) if that file is not valid JSON. Idempotent: running @[Init] twice is
a no-op the second time. @ref[#cli/lsp], @[Backrefs], @[Rename], and @[Coverage] are v1.1
subcommands and slot in under @ref[crates/context-anchors/src/commands/] (the LSP under
@ref[crates/context-anchors/src/lsp/]) with no structural change.

---

## 6. Crate choices

Versions verified on crates.io 2026-09-04. Reuse-first: the only from-scratch pieces are the
target grammar parser, the region-complement logic, the index, resolution, and grouping.

| Concern | Crate | Version | Why |
|---|---|---|---|
| Markdown | `pulldown-cmark` | 0.13.4 | byte-range `into_offset_iter`; pure Rust; rustdoc/mdBook lineage. **Deviation from @ref[DESIGN.md] (comrak)**: comrak's sourcepos is 1-based line/col and we only need byte exclusion ranges. |
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
| Release | `cargo-dist` (`dist`) | 0.32.0 | per @ref[#dist/cargo-dist]; musl targets |
| Supply chain | `cargo-deny` (`cargo-audit` subsumed) | 0.20.2 | |

Not used, deliberately: `rayon` (walker already parallel), `tree-sitter-tags` (we run the query),
`path-clean` (`RelPath::parse` does its own component walk), `bincode` (unmaintained tombstone),
`miette`/`ariadne`, `tree-sitter-loader` (dynamic loading breaks the static-binary requirement).

---

## 7. Error handling model

- `anchr-core`: every fallible fn returns `Result<T, SpecificError>` with `thiserror` enums per
  module (@ref[crates/anchr-core/src/config/config.rs#ConfigError],
  @ref[crates/anchr-core/src/text/text.rs#AnalyzeError]). No `unwrap`/`expect` in library code
  (`clippy::unwrap_used`, `clippy::expect_used` = deny at workspace level; tests are exempt via
  `#[cfg_attr(test, allow(...))]`).
- Two channels, kept apart on purpose: **tool failures** propagate as `Err` (exit 2);
  **findings about the user's files** — broken refs, malformed markers, unreadable/too-large
  files — are data inside @[Report]. A single unreadable file must never abort a check; it
  becomes an *unverified* diagnostic so it is visible but non-blocking.
- Binary: `anyhow` at the top, @ref[crates/context-anchors/src/main.rs#main] maps `Ok(report)` →
  exit code from `report`, `Err` → stderr + exit 2.

---

## 8. Concurrency and state

- Immutable inputs (@ref[crates/anchr-core/src/root/root.rs#RootSet],
  @ref[crates/anchr-core/src/config/config.rs#Config], registry) shared by `&`; per-file work is pure
  (`&str → FileScan`) and runs on the walker's threads, each owning its @[FileAnalyzer]; results
  flow over an `mpsc` channel to a single-threaded fold into @[Index]. Resolution and rendering
  are single-threaded, so their caches are plain `HashMap`s owned by the @[Resolver]. No global
  mutable state; the marker `Regex` is a `LazyLock` static and the
  @ref[crates/anchr-core/src/text/language/language.rs#LanguageRegistry] is built once per run and shared
  by `&`.

---

## 9. Testing

Every module is a directory holding its source and its tests, both named for the module:
`marker/lex/lex.rs` beside `marker/lex/lex_tests.rs`. The parent declares the module with the file
it lives in, and the module declares its own tests, which need no attribute because a `#[path]`
module's children resolve against the directory holding it:

```rust
// marker/marker.rs
#[path = "lex/lex.rs"]
mod lex;

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod marker_tests;
```

Tests stay a child module, so they keep private access; the source file stays free of them. The
crate roots are the one exception — Cargo fixes them at `src/lib.rs` and `src/main.rs`, so they pair
flat with `src/lib_tests.rs` and `src/main_tests.rs` rather than taking a directory.

The plural `_tests.rs` is load-bearing, not stylistic: `cargo-llvm-cov` excludes `*_tests.rs` and a
`tests/` directory from its report, so coverage measures production code only. A singular
`_test.rs` is *not* matched and would silently count test code as covered production lines.

- **Unit** (one `<module>_tests.rs` per module, no source file without one): @[parse_target] table tests incl. every rejection reason (qualified
  symbol, reserved chars in path segments, `.`/`..`, trailing-slash expectation, root prefix on each
  kind); @ref[crates/anchr-core/src/marker/id/id.rs#AnchorId] charset; lexer with `proptest`
  (round-trip: any generated valid marker embedded in random text is found with the right span; any
  text without `@anchor[`/`@ref[` yields none) plus a regression corpus for CRLF files and `\r` in a
  body, multi-byte text before a marker (offset and line/col both), and a non-ASCII preceding char;
  markdown regions with `insta` snapshots on fixtures covering fenced/indented code, inline code,
  headings, HTML comments, reference-style links, marker split by `[`, backslash-escaped marker not
  lexed, fence inside a blockquote and inside a list item, tilde fence, longer closing fence,
  unclosed fence at EOF, indented code inside a nested list (the complement approach's only failure
  mode is an imprecise code range letting an example through, so these are the tests that matter);
  per-language source fixtures asserting comment extraction and
  @ref[crates/anchr-core/src/text/source/source.rs#SymbolTable] contents against a **per-kind declaration
  list written first** (function, struct/class, interface, enum, type alias, method,
  const/arrow-function export, module), plus one fixture per language with deliberately broken
  syntax around a declaration asserting `Unverified::ParseErrors` rather than `SymbolMissing`;
  `LanguageRegistry::new()` succeeds.
- **Integration** (@ref[crates/context-anchors/tests/]): fixture mini-repos under
  `tests/fixtures/<case>/` with an expected `report.json` snapshot via `insta`; `assert_cmd`
  for exit codes 0/1/2, `--format json` schema stability, `--strict`, absent external root,
  duplicate anchors, cross-root refs, undeclared root ⇒ error vs absent root ⇒ unverified,
  gitignored or excluded target ⇒ missing with a note naming the cause,
  external root's refs not reported, a gitignored `.md` matching `include` is *not* scanned,
  symlink not followed, oversized file reported as unverified with the anchors-unindexed wording,
  `@ref[src/Foo.ts]` against `foo.ts` is `PathMissing` on every platform (exact-name lookup),
  `PATHS` filtering semantics, a human-output `insta` snapshot with `--color never`, a
  bad-config exit-2 snapshot showing the caret rendering, and @[Init] idempotency / `--force` /
  `--dry-run` / settings-merge preserving foreign keys.
- **Dogfood**: @ref[anchr.toml] at repo root; CI runs `anchr check --strict` over the repository.
  The design documents carry live markers: cross-document citations are anchor refs; a type or
  module mentioned repeatedly is declared once in the file's index block and written as an alias
  use at every mention; a one-off mention is a symbol or path ref. Same-document `§` citations
  stay numeric, since a renumbering is visible from inside the file being edited. The inline
  `@ref[...]` examples in those documents live in fences, which is itself a test of fence
  exclusion.
- **Scripts**: @ref[scripts/src/] holds the JS tooling and @ref[scripts/tests/] mirrors it, run with
  `node --test`. Both scripts export their pure functions and guard their CLI side effects, so a
  test can import them without fetching or writing. Repo-relative paths go through `repoRoot`,
  which finds @ref[Cargo.toml] rather than counting `..` segments.
- **Coverage floor**: CI runs `cargo llvm-cov` and fails under 88% workspace lines or 70% on any
  single file. Both ratchet upward as gaps close; neither is satisfiable by a test that asserts
  nothing, which is why coverage rather than a per-file test mandate enforces sufficiency.
- **Security gates in CI**: `cargo deny check`, `cargo audit`, `cargo clippy -D warnings`,
  `cargo test`, plus `cargo build --release` size check for the binary (grammar bundle budget).

---

## 10. Security posture (from the corgea checklist)
<!-- @anchor[code/security] -->

Threat model: every walked file, every config file, and every marker body is untrusted input. A
hostile repository must not be able to crash the checker, read outside its roots, or exhaust
memory. Concrete commitments, mapped to the checklist items they satisfy:

**Workspace lints and profile** (items 1, 2, 7, 9, 10, 11, 15, 24) — declared once in the
root @ref[Cargo.toml], every crate opts in with `lints.workspace = true`:

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

`#[must_use]` on @[Report], @ref[crates/anchr-core/src/resolve/resolve.rs#Resolution], and every `parse`
constructor. **No** `#[non_exhaustive]` on core enums: the binary crate matches @[DiagnosticKind],
@ref[crates/anchr-core/src/resolve/resolve.rs#Unresolved], and
@ref[crates/anchr-core/src/resolve/resolve.rs#Unverified] exhaustively in both renderers, and
`non_exhaustive` (which applies across crates) would force `_ =>` arms there and defeat the "add a
variant, every match fails to compile" property we want. It goes only on the JSON DTO enums that
external consumers deserialize.

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
escapes the root yields `Unresolved::PathEscapesRoot` rather than a read. Existence-only path refs
consult the scan tree (§3.5); a symlink entry is redirected lexically and never dereferenced,
except that a link whose target leaves the root is classified with `fs::metadata` for existence
only. Nothing is read without the canonical containment check. The walker runs with
`follow_links(false)`. Functions take `&Utf8Path`, not owned paths. A `proptest` asserts
`RelPath::parse(s).map(|p| root.join(p))` never escapes `root` for arbitrary `s`, and a second
one asserts the same of `RelPath::anchored(dir, s)` over `(dir, s)` pairs: `./` and `../`
spellings are normalized before construction, so a @[RelPath] is root-relative regardless of
spelling.

**Input bounds** (items 14, 16, 19) — @ref[crates/anchr-core/src/marker/id/id.rs#AnchorId],
@ref[crates/anchr-core/src/root/root.rs#RootName],
@ref[crates/anchr-core/src/marker/symbol/symbol.rs#SymbolName], and
@[RelPath] are allowlist-parsed newtypes with private fields
and length caps (ID ≤ 256 bytes, segment ≤ 64, path ≤ 1024). File size is checked from metadata
before reading (`max-file-bytes`, default 2 MiB, validated ≤ `u32::MAX`); oversized files are
unverified findings. The marker regex is `regex` (linear time). Tree-sitter parsing runs under a
progress-callback budget (`ParseTimeout`), and @[SymbolTable] construction caps at 100k declarations
per file, discarding the table and reporting `SymbolTableTruncated` rather than returning a partial
one. No `with_capacity(n)` where `n` derives from file content. Config: `deny_unknown_fields`, enums
for every fixed-choice field (`UnverifiedPolicy { Report, Error }`,
@ref[crates/anchr-core/src/marker/path/path.rs#PathExpectation],
@ref[crates/anchr-core/src/scan/scan.rs#ScanMode], never a bool), collection caps validated right after
deserialization, `~` expansion only on `[roots]` values.

**Integer handling** (item 15) — all offsets are `usize` internally; conversion to `u32` for JSON
and LSP goes through `u32::try_from` and a @ref[crates/anchr-core/src/span/span.rs#PositionOverflow]
error. `&str` slicing at file-derived offsets uses `get(..)`, never `[..]`, because a tree-sitter or
pulldown offset that lands mid-codepoint would otherwise panic.

**No shell, bounded output** (items 18, 21) — no `std::process::Command` anywhere in milestone 1:
git toplevel detection walks up for a `.git` entry instead of invoking `git`. JSON output
contains only paths, spans, marker bodies, IDs, and suggestions, never file content. The human
renderer does print source lines, but only the lines containing a marker site in a file the
scan already selected (a `.env` is not a scanned container), and only for the first site of
each diagnostic.

**Supply chain** (item 3) — grammar crates compile C in `build.rs`, so each is vetted (repo,
maintainer, download count) before adding; @ref[deny.toml] per the site's template with
`unknown-git = "deny"`, `wildcards = "deny"`, license allowlist
MIT/Apache-2.0/BSD-3/ISC/Unicode-3.0; @ref[Cargo.lock] committed; CI builds with `--locked`;
`cargo audit` + `cargo deny check` on every PR; `cargo geiger` run once to record the unsafe budget the tree-sitter stack brings in.

**Gaps the checklist does not cover, handled anyway** — symlink loops: the walker never follows
links, lexical redirection in the scan tree is bounded by `MAX_SYMLINK_HOPS`, and the only reads
outside the walk are canonicalized symbol-resolution targets. Open-file
pressure: the walker's thread count bounds concurrent reads and each file is read whole and
dropped before the next. Char boundaries: covered under integer handling above. JSON output:
`serde_json` escapes control characters; invalid UTF-8 cannot reach it because non-UTF-8 files
are rejected at read time.

**Testing** (item 23) — `cargo fuzz` targets for @[parse_target], `lex`, `markdown::text_regions`,
`source::text_regions` (per language), and `Config::from_str`: must never panic or hang.
`proptest` on the lexer and @[RelPath]. CI: `fmt --check`, `clippy -D warnings`, `test --locked`,
`audit`, `deny check`, MSRV job.

---

## 11. Build order

The version labels in @ref[#design/scope] are milestones, not releases: everything below is built in
succession before the first public release, so the core is designed for the final shape from
step 1 (the @[Index] update/remove API, the DTO-decoupled JSON schema, and the
@ref[crates/anchr-core/src/text/language/language.rs#LanguageSpec] table all exist because steps 9–11 need
them).

**Milestone 1 — the guarantee (`anchr check`)**
1. Workspace scaffold, lints, @ref[deny.toml], CI skeleton, `cargo-dist` config. Two spike tests
   that pin third-party behaviour the design depends on: a walker-thread panic propagates to
   the caller, and a gitignored file is not yielded even when it matches an include glob.
2. @ref[crates/anchr-core/src/span/span.rs], @ref[crates/anchr-core/src/marker/] (types +
   @[parse_target] + lexer) — pure, fully unit-tested first.
3. @ref[crates/anchr-core/src/text/markdown/markdown.rs], plaintext regions
   (@ref[crates/anchr-core/src/text/text.rs]), @ref[crates/anchr-core/src/text/language/language.rs] +
   @ref[crates/anchr-core/src/text/source/source.rs] (one language first: Rust, to dogfood; then
   TS/TSX/JS/Py/Go).
4. @ref[crates/anchr-core/src/config/config.rs], @ref[crates/anchr-core/src/root/root.rs],
   @ref[crates/anchr-core/src/scan/scan.rs], @ref[crates/anchr-core/src/index/index.rs].
5. @ref[crates/anchr-core/src/resolve/], @ref[crates/anchr-core/src/suggest/suggest.rs],
   @ref[crates/anchr-core/src/diagnostic/diagnostic.rs], @ref[crates/anchr-core/src/check/check.rs#run_check].
6. Binary: @ref[crates/context-anchors/src/cli/cli.rs], the @[Check] command, human + JSON render,
   exit codes; integration fixtures.
7. @[Init]; dogfood @ref[anchr.toml]; fuzz targets.
8. Release pipeline: cargo-dist (shell/powershell installers, musl targets), plus the npm
   platform-package script (`@context-anchors/<platform>` + thin `anchr` shim generated from
   `dist-manifest.json`), dry-run against a pre-release tag.

**Milestone 2 — the multiplier**
9. `anchr lsp` (`lsp-server` + `ls-types`, stdio): diagnostics on open/change, go-to-definition
   for `#id` (anchor @[Site]) and `file#Symbol` (@[SymbolTable] declaration spans),
   find-references via `Index::backrefs`, rename of an anchor ID via the recorded `id_span`s,
   document symbols. Reuses `check::run_check` machinery per root and `Index::update_file` per
   edited document; `catch_unwind` per request (§10).
10. `anchr backrefs`, `anchr rename` (rename is the only mutating command besides @[Init]; it
    rewrites exactly the `id_span` bytes of every anchor and ref carrying the old ID and prints
    every file touched).
11. `anchr coverage` / `anchr annotate`: heuristic scanner over the same text regions, reporting
    reference-shaped strings that are not annotated; never errors, never writes on its own.
    `@noref` and `[ignore] tokens` retire the candidates an author has judged (§12a items 14
    and 20).

**Milestone 3 — review ledger and ecosystem** (design pass required before code)
12. `anchr review` / `anchr accept` signature ledger; exported vs. internal anchors; MCP adapter
    if warranted. These need their own code-level design; out of scope for this plan.

Each numbered step is one PR-sized unit and independently testable.

---

## 12. Decisions that deviate from @ref[DESIGN.md] / @ref[DISTRIBUTION.md]

All approved with the design; each is independent, so any one can be reversed without
disturbing the rest.

1. **pulldown-cmark instead of comrak** — byte ranges natively; see §6.
2. **No persistent index in v1** — see §3.4; survey independently reached the same conclusion.
3. **`root:` prefix allowed on all target kinds**, not only `#id` — see §2.
4. **Human renderer is annotate-snippets**, so the report is rustc-shaped (one snippet + site
   list) rather than the pure `path:line` list sketched in @ref[#design/diagnostics].
5. **npm channel conflict.** @ref[#dist/channels] requires the esbuild-style
   `optionalDependencies` platform-package layout and explicitly rejects a postinstall download
   shim. cargo-dist 0.32's npm installer *is* a download shim (one package, fetches the archive
   from GitHub Releases at install time). Options: (a) ship the shim in v1 and revisit; (b) write
   a small release-workflow script that builds the platform packages from cargo-dist's
   `dist-manifest.json` (esbuild/biome pattern; roughly a day of work); (c) defer npm to v1.1 and
   ship curl + GitHub Releases only. **Decision: (b).** The user wants the final shape, and all
   milestones are built before anything is released, so there is no interim to optimize for.
   @ref[DISTRIBUTION.md] §4 stands as written.
6. **LSP stack for v1.1**: `lsp-server` + `ls-types`, synchronous, negotiating
   `positionEncoding: utf-8` with UTF-16 fallback via `line-index`. Not tower-lsp (dead) or
   tokio-based servers (unneeded for a re-parse-whole-document server).

## 12a. Implementation notes (milestones 1 and 2 built)

Refinements the code made to the design above, recorded so the document stays the map:

1. **Two path types.** @[RelPath] is the reference grammar's allowlisted path. Scanned files
   get their own identity, @[FilePath], which accepts any UTF-8 relative name: `My Notes.md`
   must be scanned even though no `@ref` can name it. @[Site] and the index are keyed by
   @[FilePath]; @[RelPath] converts into it for resolution.
2. **File identity lives on the scan, not the marker.**
   @ref[crates/anchr-core/src/marker/marker.rs#Marker] carries spans only; @[Site] (root, path,
   span, region) is built by the index and diagnostics. Markers keep a
   `body_span`, and refs to anchors an `id_span`, which rename and the LSP rewrite.
3. **Comments exclude backtick spans**, mirroring markdown's inline-code rule (§3.1). This
   repository's own doc comments needed it to pass `anchr check`.
4. **A leading backslash escapes a marker** (`\@ref[...]`), alongside the bracket-escape form.
5. **The lexer's body class excludes `[`**, so an unclosed opener cannot swallow the next
   marker; each opener gets its own diagnostic.
6. **@[Coverage] sees more than @[Check] lexes**: markdown code spans (`RegionKind::InlineCode`)
   and raw comment nodes, minus link destinations and fences. A code span is a candidate only
   when its whole content is one path (item 16). A path token is a candidate when it ends in `/` (a
   `/*` or `/**` tail counts and is proposed as the directory) or its last segment is
   `stem.ext` with an extension from the table in item 15; the stem must contain a letter and
   the pieces cannot all be single characters, so `line/col`, `Apache-2.0/MIT`, `v1.1`, and
   `e.g.` are never candidates.
7. **@ref[crates/anchr-core/src/check/check.rs#Workspace]** owns the registry, the indexed roots, and
   scan findings; @[Check], @[Backrefs], @[Rename], @[Coverage], and the LSP all run against it.
   `Workspace::update_file` re-lexes one document from editor text for the LSP.
8. **LSP stack**: `lsp-server` + `ls-types`, synchronous, `catch_unwind` per message, full
   document sync, UTF-8 positions when the client offers them. The connection is dropped before
   the stdio threads are joined; the writer thread does not exit otherwise.
9. **npm**: @ref[scripts/src/npm/build-packages.mjs] builds `@context-anchors/<os>-<cpu>` packages
   (static musl on Linux) plus the `context-anchors` shim from cargo-dist's manifest, published
   by @ref[.github/workflows/publish-npm.yml] as a dist custom publish job. That workflow's three
   jobs share the index written by @ref[scripts/src/npm/packages-index.mjs]:
   @ref[scripts/src/npm/verify-packages.mjs] installs the packed tarballs on a runner per platform
   and @ref[scripts/src/npm/publish-packages.mjs] publishes them, shim last and skipping versions
   already on the registry. Orchestration lives in these scripts rather than in the workflow
   because npm reads a bare `owner/repo`-shaped argument as a git shorthand — the bug that stopped
   the 0.0.1 shim publishing — and because the verification has to run on Windows too.
10. **A narrow `catch_unwind` guards pulldown-cmark.** Fuzzing found that its offset iterator
    panics on some malformed documents (pulldown-cmark/pulldown-cmark#1129, open upstream).
    §10's rule against catching panics around per-file work still holds for our own code: the
    catch wraps only the third-party iteration and turns a panic into
    `AnalyzeError::ParserPanicked`, reported as an unverified skipped file. The fuzz target
    restores the default panic hook so the catch is exercised under fuzzing too.
11. **Names that differ from the body above.**
    @ref[crates/anchr-core/src/text/text.rs#AnalyzeError] is the design's `ContainerError`.
    There is no `NoSymbolQuery`: every registered language ships a declaration query. The
    `DirectoryListingCache` became the scan-produced
    @ref[crates/anchr-core/src/tree/tree.rs#FileTree] (item 18). Skipped files are one
    `DiagnosticKind::FileSkipped` carrying a
    @ref[crates/anchr-core/src/scan/scan.rs#SkipReason] rather than `FileNotUtf8`/`FileTooLarge`, and
    @ref[crates/anchr-core/src/resolve/resolve.rs#Unresolved] gained `PathNotDirectory`/`PathNotFile`
    for the trailing-slash rule. Module layout: @ref[crates/anchr-core/src/marker/] is split into
    @ref[crates/anchr-core/src/marker/id/id.rs], @ref[crates/anchr-core/src/marker/path/path.rs],
    @ref[crates/anchr-core/src/marker/symbol/symbol.rs], and
    @ref[crates/anchr-core/src/marker/target/target.rs]; plaintext regions live in
    @ref[crates/anchr-core/src/text/text.rs]; anchors resolve inside
    @ref[crates/anchr-core/src/resolve/resolve.rs]; @ref[crates/anchr-core/src/edit/edit.rs],
    @ref[crates/anchr-core/src/rename/rename.rs], and @ref[crates/anchr-core/src/coverage/] are new.
    The section headings above point at the real modules.
12. **Milestone 3 is not started.** `review`/`accept`, exported anchors, and MCP need their own
    code-level design before implementation.
13. **Alias imports.** Dogfooding showed that qualified symbol refs are too long to write at
    every mention, so unannotated mentions rot unseen. The fix borrows `import x as y`: a file
    declares a target once under a local name (`@ref[target as X]`) and writes `@[X]` at every
    mention. The alias table lives on the file's index record; uses bind through it, join
    @[Backrefs], and are counted separately; undeclared and duplicate aliases are errors keyed by
    file. Grammar, semantics, and reasons are in @ref[docs/design/aliases.md]. This document's own
    index block at the top is the first user.
14. **@[Coverage] ignores.** Once the repository was annotated, most remaining coverage candidates
    were classified correctly and still were not references: example paths, files that exist in
    a user's repository. `@noref[a, b/]` declares them per file and a root-wide list in
    @ref[anchr.toml] per root; both share one matcher
    (@ref[crates/anchr-core/src/noref/noref.rs#NoRefSet]) and every entry that matches nothing is
    reported, the way an unused alias is. @[Check] lexes the marker and otherwise never sees it.
    Design in @ref[docs/design/ignores.md]. The config shape and the matcher's exact-plus-prefix
    rule were superseded by item 20.
15. **Extensions come from GitHub Linguist plus the root itself.** Dogfooding showed every
    unresolvable coverage row was a prose slash pair, so a `/`-token now needs a directory tail
    or a real extension. @ref[crates/anchr-core/src/coverage/linguist/linguist.rs] is Linguist's extension
    list at a pinned commit, generated by @ref[scripts/src/linguist/gen-extensions.mjs] (multi-dot,
    all-digit, and non-ASCII entries dropped; the header records the counts) and kept fresh by a
    CI job that regenerates from the pin and fails on a diff. The scan records the extension of
    every file it walks past, parsed or not, and the union is what coverage consults: an in-house
    format stays a candidate while one such file exists, and a `.txt` mention stays reported after
    the last `.txt` file is renamed. The known leaks are domains whose TLD is a language
    (`crates.io`) and extensions that double as method names once such a file exists
    (@ref[Cargo.lock]); both are correct shapes for `[coverage] ignore`. On this repository the
    unresolvable bucket went from 32 rows, all prose, to 0.
16. **Bare code symbols are not coverage candidates.** With paths precise, every remaining row on
    this repository (48) was a backticked identifier placed by name against a flat index of every
    declaration in the root: 22 were parameters, fields, or sibling methods named in their own
    doc comment, 15 of those proposing an unrelated same-named declaration that @[Check] would
    have accepted; 15 were module and subcommand names used as words; 6 duplicated rustdoc links;
    1 was a real miss. A name has no single referent, so the tool cannot know which declaration
    the author meant, and the collision rate grows with the codebase. Language doc links are no
    fallback: of the five supported languages only rustdoc checks them, and only under
    `cargo doc`. The symbol index and identifier tokens are gone from coverage; @[Check] and the
    LSP keep resolving `file#Name` through @ref[crates/anchr-core/src/resolve/symbol/symbol.rs]. Symbols
    enter coverage through an alias declaration, after which every use in that file is proposed,
    unique by construction. @[Coverage] no longer parses every source file per run. On this
    repository: 307 of 355 annotated, 27 could be, 21 ambiguous → 307 of 307, no candidates.

17. **@[Coverage] groups by token, the way @[Check] groups by cause.** Run against a 14,000-file
    monorepo, the flat report was 2,935 lines, and 1,762 of them were one token: a header comment
    in every generated type file. Nobody reads past the first screen of that, human or agent. The
    report is now one group per (verdict, token) with its sites listed beneath it, in both
    formats: the human form is a title line, ` --> path:line:col` per site, and the same 40-site
    cap @[Check] uses; the JSON form is one object per group with `count` and `sites`, each site
    keeping its exact text because a proposal's edit must match bytes. Groups are ordered
    proposals, unresolvable, unused aliases, unused ignores, then by site count descending, then
    by token, so the largest actionable item is always first. Grouping happens in the core
    (@ref[crates/anchr-core/src/coverage/coverage.rs#CandidateGroup]), not the renderer, so every
    consumer sees the same shape. Summary counts still count sites. On the monorepo: 2,935 lines
    → 1,937, and the JSON candidate list 2,934 entries → 724 groups. Most groups have one site,
    so the human report only halves; the point is that the thousand-site group is now one entry
    at the top of its kind instead of the whole report.

18. **A target exists only if the scan enumerated it.** On the monorepo, @[Coverage] proposed
    `@ref[server/lib/src/modules/invoices/invoice-cover-page.js]`: build output in a gitignored
    directory, present because the build had run. Existence came from `read_dir`, so
    @[Check] would have passed locally and failed on a clean checkout, the exact split invariant
    5 forbids. The walk now records every file and symlink it yields in a
    @ref[crates/anchr-core/src/tree/tree.rs#FileTree] carried by the @[Index], and path resolution is
    a lookup in that tree (§3.5): gitignored and config-ignored paths are missing, an empty
    directory is missing, and a missing path that is on disk gets a note naming the pattern or
    the ignore rules. At the time this left three knobs meaning three things (gitignore and
    `[scan] exclude` removed a path from existence, indexing, and coverage; `[coverage] exclude`
    only stopped proposals); item 20 collapsed them to one, `[ignore] paths`, with the first
    meaning. Ignore a tree only when nothing should reference into it. Symlinks are
    redirected lexically inside the root and fall back to the disk only when they leave it;
    `EntryKind::Other` is gone because a socket is simply not in the tree. The one deliberate
    LSP caveat: an editor buffer for a gitignored file counts as present for that session; the
    CLI is the CI truth.

19. **File-relative targets.** Of the monorepo's 971 unresolvable coverage rows outside its
    generated tree, 229 resolved relative to the file that wrote them and 146 relative to an
    ancestor directory: skills and package READMEs name their own files the way Markdown links
    do. Rewriting those to root-relative paths would pin a skill directory to one location in
    one repository, and skill directories are copied between repositories. So the grammar gained
    `./` and `../` (§2), anchored in the lexer so nothing downstream changes. @[Coverage] uses the
    same fact in its fallback (@ref[crates/anchr-core/src/coverage/coverage.rs#relative_fallback]): a
    bare token that misses at the root is proposed as `./token` when it resolves beside its file,
    as the root-relative path when exactly one ancestor directory resolves it, and otherwise
    stays unresolvable with a hint naming the one file of that basename if there is one. @[Check]
    adds a per-site note when a missing bare path exists beside the file that wrote it; a note
    rather than the suggestion because the suggestion is per cause and the same name may sit
    beside one file and nowhere near another. A token that is only dots and slashes (`./`, `../`)
    is never a candidate: it names the writing file's own directory, which always exists, and in
    prose it is a mention of the syntax. Rejected: resolving bare paths with a root-then-file
    fallback and no prefix, because a target string would stop having one meaning and `check`
    results would change when an unrelated file appeared. In this repository only three markers
    point at a sibling; the rest point up and across and stay root-relative. On the monorepo:
    proposals 201 → 594 (233 sites proposed as `./`, 148 under exactly one ancestor), and 1,983
    of the still-unresolvable sites now name the one file of that basename.
20. **One `[ignore]` table.** Explaining the ignore surface after the monorepo run exposed five
    knobs for two questions: `.anchrignore`, `[scan] exclude`, `[coverage] exclude`,
    `[coverage] ignore`, and `@noref`, named after the phase that read them, with
    `[scan] exclude` compiled twice under two syntaxes (globset for the "why is this missing"
    note, gitignore for the walk) and a token rule nobody could see in the config: an entry
    ending in `/` matched every token beneath it, so this repository's own `src/` entry hid
    every `src/...` mention and `.claude/` would have hidden every skill. The config is now
    `[ignore] paths` (gitignore syntax, layered on `.gitignore`, one `Gitignore` matcher shared
    by the walker's `filter_entry` and the resolver's note) and `[ignore] tokens` (the same
    literal globs `@noref` takes: `src/` is only `src/`, `src/**` is the subtree, `**/x` is any
    depth). `.anchrignore` is gone, and so is "checked but never proposed": a file is looked at
    or it is not, and the archived research that relied on it is annotated and `@noref`ed
    instead (16 proposals, 11 non-references). Full globset is accepted everywhere; a marker
    body simply cannot spell `[`, `]`, or a bare comma. In the same change `.gitignore` handling
    became git's own (`require_git(true)`): honoured only inside a repository, parent files
    stopping at the nearest `.git`, after a copy of this repository under `~/.claude` lost
    @ref[docs/research/] to an unanchored `research/` line in `~/.claude/.gitignore`; ripgrep
    `.ignore` files stopped being read. Design and the superseded reasoning in
    @ref[docs/design/ignores.md] §8.

## 13. Research appendix

The research this design rests on lives in @ref[docs/research/]:

- @ref[docs/research/rust-security-best-practices-digest.md] — corgea checklist (25 items) with a
  mapping onto a filesystem-walking, tree-sitter, TOML-config, JSON-emitting CLI. §10 is derived
  from it.
- @ref[docs/research/crate-survey.md] — 15 areas, versions verified on crates.io 2026-09-04, API
  sketches, gotchas. §6 is derived from it.
- @ref[docs/research/code-design-review.md] — 25 findings against the draft of this document, all
  incorporated. The three blockers were: include globs as `ignore` overrides bypassing `.gitignore`,
  false `SymbolMissing` on error-bearing parse trees, and case-insensitive path resolution making
  @[Check] platform-dependent.

Survey caveats to verify during implementation: compiled grammar sizes are estimates; `ignore`
override-vs-gitignore precedence is pinned by a spike test in build step 1 rather than assumed.
