# Rust crate landscape for `anchr` / `context-anchors` (verified 2026-09-04)

All versions below were checked against the crates.io API on 2026-09-04 unless marked **[unverified]**.

---

## 1. CommonMark parsing

| Crate | Version | Last release | Health |
|---|---|---|---|
| pulldown-cmark | 0.13.4 | 2026-05-20 | Active, 146M downloads, regular patch cadence |
| comrak | 0.54.0 | 2026-07-12 | Very active (100 releases), MSRV 1.85 |
| markdown (markdown-rs) | 1.0.0 | 2025-04-23 | Stable but quiet: no commits since the 1.0.0 tag in April 2025 |

**Source positions, the part that matters for you:**

- **pulldown-cmark**: `Parser::new_ext(src, opts).into_offset_iter()` yields `(Event, Range<usize>)` where the range is a **byte range into the source**. Exactly what you want. `Event::Text`, `Event::Code` (inline code span), `Event::Start(Tag::CodeBlock(..))`/`Event::End(TagEnd::CodeBlock)`, `Event::InlineHtml`, `Event::Html` are all distinct, so skipping code spans and fenced blocks is a matter of a small state flag while iterating.
- **comrak**: `node.data.borrow().sourcepos` is `Sourcepos { start: LineColumn, end: LineColumn }`, both **1-based**, and column is counted in **UTF-8 bytes** by default (a `parse.sourcepos_chars` option switches to chars). So to get a byte offset you build a newline table and compute `line_start[line-1] + (column-1)`. Workable but it is a second index and it makes every text node a two-step lookup. Also `end` is inclusive ("the last character"), so the byte range is `[start, end+1)`.
- **markdown-rs**: `to_mdast()` gives a `Node` tree with `unist::Position { start: Point, end: Point }` and `Point.offset` is a **0-indexed character offset, not a byte offset** per the docs. That is a trap for a byte-oriented lexer on non-ASCII prose; you would have to convert.

**Gotchas for pulldown-cmark specifically:**

- Adjacent `Text` events are split at backslash escapes, entity references, soft breaks and some other boundaries, so `@anchor[id]` could straddle two events. Use `pulldown_cmark::utils::TextMergeWithOffset` (exists in 0.13, "merge consecutive Text events into only one, with offsets") **[exact signature unverified, the docs page is thin]**, or simpler: collect runs of consecutive text ranges yourself and lex the **source slice** `&src[range]` rather than the event's `CowStr`. Lexing the source slice is the right call anyway because the `CowStr` content is unescaped and no longer aligns with source offsets.
- Merging across `SoftBreak` is a design decision: a marker split across a line break should probably be treated as not-a-marker, so merge only directly adjacent `Text` ranges.
- Enable only what you need: `Options::ENABLE_TABLES | ENABLE_FOOTNOTES | ENABLE_STRIKETHROUGH | ENABLE_TASKLISTS | ENABLE_GFM` is a sane GitHub-ish set. Do not enable `ENABLE_SMART_PUNCTUATION` (it rewrites text and adds more splits). Skip `ENABLE_WIKILINKS` unless you want `[[..]]` parsed, which could interact with `@ref[...]` syntax.

**Recommendation: pulldown-cmark.** Native byte ranges, pull-based so no arena/RefCell, smallest dependency footprint, most downloads, actively released. Comrak is the right choice only if you need round-trip formatting or its GFM extension breadth; markdown-rs loses on char-vs-byte offsets and stalled activity.

```rust
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

fn text_byte_ranges(src: &str) -> Vec<std::ops::Range<usize>> {
    let mut ranges: Vec<std::ops::Range<usize>> = Vec::new();
    let mut in_code_block = false;
    for (event, range) in Parser::new_ext(src, Options::ENABLE_GFM | Options::ENABLE_TABLES).into_offset_iter() {
        match event {
            Event::Start(Tag::CodeBlock(_)) => in_code_block = true,
            Event::End(TagEnd::CodeBlock) => in_code_block = false,
            Event::Text(_) if !in_code_block => match ranges.last_mut() {
                Some(last) if last.end == range.start => last.end = range.end,
                _ => ranges.push(range),
            },
            _ => {}
        }
    }
    ranges
}
```

---

## 2. tree-sitter from Rust

| Crate | Version | Last release |
|---|---|---|
| tree-sitter | 0.27.0 | 2026-08-30 |
| tree-sitter-language | 0.1.8 | 2026-08-30 |
| tree-sitter-tags | 0.27.0 | 2026-08-30 |
| tree-sitter-loader | 0.27.0 | 2026-08-30 |
| tree-sitter-rust | 0.24.2 | 2026-03-27 |
| tree-sitter-javascript | 0.25.0 | 2025-09-01 |
| tree-sitter-python | 0.25.0 | 2025-09-11 |
| tree-sitter-go | 0.25.0 | 2025-08-29 |
| tree-sitter-typescript | 0.23.2 | 2024-11-11 |
| tree-sitter-md | 0.5.3 | 2026-02-26 |

**ABI compatibility (the historical pain point is largely solved):**

- tree-sitter 0.27.0 has `LANGUAGE_VERSION = 15` and `MIN_COMPATIBLE_LANGUAGE_VERSION = 13`. Every grammar crate above was generated with ABI 14 or 15, so all load fine.
- Since tree-sitter 0.23, grammar crates no longer depend on `tree-sitter` at all. They depend on `tree-sitter-language ^0.1` and export a `LanguageFn` constant. The `tree-sitter` crate implements `From<LanguageFn> for Language`, so `parser.set_language(&tree_sitter_rust::LANGUAGE.into())` works regardless of which 0.2x tree-sitter you pin. Grammar crates only list `tree-sitter` as a dev-dependency, which does not affect you. Result: **you can pin tree-sitter 0.27 and take whatever grammar versions exist**, and `Language::abi_version()` lets you assert at startup if you want a guard.
- One exception: `tree-sitter-md` has an optional `parser` feature that depends on `tree-sitter ^0.26`. Do not enable it, or you get two copies of the runtime. You only need its `LANGUAGE` / `INLINE_LANGUAGE` constants anyway, and you probably do not need tree-sitter-md at all given pulldown-cmark handles Markdown.
- tree-sitter-typescript is the stalest grammar (crate Nov 2024, repo's last commit Jan 2025). It still works (ABI 14), but do not expect newer TS syntax. Exports `LANGUAGE_TYPESCRIPT` and `LANGUAGE_TSX` as two separate parsers.

**Query API in 0.27 (changed since 0.24, watch old blog posts):**

- `Query::new(&language, source) -> Result<Query, QueryError>`.
- `QueryCursor::matches(&mut self, &query, node, text_provider)` and `captures(...)` return **streaming iterators**; you must `use tree_sitter::StreamingIterator` and loop with `while let Some(m) = matches.next()`. They are not `Iterator`. The `text_provider` is typically `source.as_bytes()`; it exists because `#eq?` / `#match?` text predicates are evaluated by the Rust bindings against it.
- `Node::byte_range() -> Range<usize>`, `Node::kind()`, `Node::utf8_text(source)`.
- 0.27 breaking notes relevant to you: `Node::child_count` now returns `u32`, query iterators are tied to the options lifetime, parser loggers must be `Send + 'static`. Minor for a fresh codebase.

**tags.scm reuse: yes, and it is exposed.** Every grammar crate you listed exports `TAGS_QUERY: &str` (rust, typescript, javascript, python, go). Contents are exactly what you want for "does a named declaration exist":

- Rust: `(struct_item name: (type_identifier) @name) @definition.class`, `(function_item name: (identifier) @name) @definition.function`, `(trait_item ...) @definition.interface`, `(mod_item ...) @definition.module`, methods inside `declaration_list` as `@definition.method`, `enum_item`/`union_item` as `@definition.class`.
- TypeScript: `function_signature`, `abstract_class_declaration`, class/interface/method definitions with `@name`. Note the TS tags file does **not** inherit the JS one, so `function_declaration` from JS is covered separately in TS's own file (verify per-kind when you write the mapping table).
- Python: `class_definition` and `function_definition` with `@name`.
- Go: `function_declaration` → `definition.function`, `method_declaration` → `definition.method`, `type_spec` → `definition.type`.

Two ways to consume them:

1. **`tree-sitter-tags` crate**: `TagsConfiguration::new(language: Language, tags_query: &str, locals_query: &str)` then `TagsContext::generate_tags(&config, source, None)` yields `Tag { range, name_range, syntax_type_id, is_definition, docs, .. }` with `config.syntax_type_name(id)` giving `"function"`, `"class"`, etc. Convenient, but it pulls in `regex` and `memchr`, and its doc coverage is 17%. Fine for an MVP.
2. **Run `TAGS_QUERY` yourself** with `Query::new(&lang, TAGS_QUERY)` and iterate matches, using `query.capture_names()` to find the `@name` and `@definition.*` capture indices. About 40 lines, no extra crate, and you get to filter to `definition.*` patterns only via `disable_pattern` on the `reference.*` ones. **Recommended**: it is one API instead of two, and finding comments is the same mechanism.

Comments: there is no shared `comments.scm`, but the node kinds are stable: Rust `line_comment` / `block_comment`, JS/TS `comment`, Python `comment`, Go `comment`. A one-line query per language, e.g. `[(line_comment) (block_comment)] @comment`, is all you need. Rust doc comments (`///`) are `line_comment` nodes with a `doc_comment` child in tree-sitter-rust 0.24; `byte_range()` of the outer node covers the whole thing.

```rust
use tree_sitter::{Parser, Query, QueryCursor, StreamingIterator};

let mut parser = Parser::new();
parser.set_language(&tree_sitter_rust::LANGUAGE.into())?;
let tree = parser.parse(source, None).ok_or(ParseTimedOut)?;

let query = Query::new(&tree_sitter_rust::LANGUAGE.into(), tree_sitter_rust::TAGS_QUERY)?;
let name_idx = query.capture_index_for_name("name").expect("tags.scm always has @name");
let mut cursor = QueryCursor::new();
let mut matches = cursor.matches(&query, tree.root_node(), source.as_bytes());
while let Some(m) = matches.next() {
    let kind = &query.capture_names()[m.captures.iter().find(|c| c.index != name_idx).unwrap().index as usize];
    if !kind.starts_with("definition.") { continue; }
    let name_node = m.captures.iter().find(|c| c.index == name_idx).unwrap().node;
    let name = name_node.utf8_text(source.as_bytes())?;
    // record (kind, name, name_node.byte_range())
}
```

**Dynamic loading vs static:** `tree-sitter-loader` compiles and `dlopen`s grammars at runtime, needs a C compiler on the user's machine, and breaks the "single static binary" requirement. Static linking a handful of grammars is the sane default and is what every serious Rust tool does (Helix vendors them, Zed statically links). Requires a C compiler **at build time** only (`cc` crate); cargo-dist's CI runners have one.

**Binary size per grammar** (compiled from `parser.c` sizes on GitHub master, cross-checked against nvim-treesitter `.so` sizes reported by users): **[estimate]**

| Grammar | parser.c source | Compiled, approx |
|---|---|---|
| typescript | 8.7 MB | ~1.1 MB |
| tsx | ~8.7 MB | ~1.2 MB |
| rust | 6.5 MB | ~0.8 MB |
| javascript | 2.9 MB | ~0.5 MB |
| python | 3.4 MB | ~0.5 MB |
| go | 1.6 MB | ~0.3 MB |
| tree-sitter runtime | | ~0.4 MB |

So the full set adds roughly 5 MB to the binary. Acceptable. TSX is the one to consider making optional; you get TS and TSX as two parsers from the same crate.

**`unsafe`:** the `tree-sitter` crate wraps all FFI; grammar crates expose only a `LanguageFn` const. Your crate needs zero `unsafe`, so `#![forbid(unsafe_code)]` is feasible.

---

## 3. Filesystem walking

- **ignore 0.4.33** (2026-08-04), **globset 0.4.20** (2026-08-04). Both are ripgrep crates, released together, very healthy.
- `WalkBuilder::new(root).hidden(true).git_ignore(true).git_global(true).git_exclude(true).require_git(false).overrides(ov).build_parallel()`, then `.run(|| Box::new(|entry| { ...; WalkState::Continue }))`. The closure factory runs once per thread, so per-thread parsers (tree-sitter `Parser` is not `Sync`) fit naturally. Use `.threads(n)` or leave default (available parallelism).
- Set `require_git(false)` so `.gitignore` files are honoured even outside a git checkout (default is `true`, which silently ignores `.gitignore` when there is no `.git`). Consider `add_custom_ignore_filename(".anchrignore")`.
- **Overrides**: `OverrideBuilder::new(root).add("**/*.md")?.add("!vendor/**")?.build()?`. Semantics are gitignore lines with `!` inverted: bare globs are whitelist (include), `!` globs exclude. Key rule from the docs: "if there is at least one whitelist override and `is_dir` is false, then this never returns `Match::None`, since non-matches are interpreted as ignored", so a single include glob turns the walk into include-only for files while still descending directories. Overrides are consulted before ignore files and win when they match (this is ripgrep's `-g` behaviour, "always overrides any other ignore logic") **[from memory of `ignore::dir::Ignore::matched`, not re-read today]**.
- `ignore` already depends on `globset`, so use `globset` for any config-driven matching of your own (e.g. deciding which language a path is, or `[[rules]] paths = [...]` in config). `GlobSetBuilder` + `GlobSet::matches(path)`; enable `literal_separator(true)` on each `GlobBuilder` so `*` does not cross `/`.

---

## 4. CLI

- **clap 4.6.6** (2026-08-06), derive feature. **clap_complete 4.6.9** (2026-08-06).
- Static completions: `clap_complete::aot::generate(shell, &mut Cli::command(), "anchr", &mut io::stdout())` in an `anchr completions <shell>` subcommand; `Shell` enum covers bash, zsh, fish, powershell, elvish (nushell via `clap_complete_nushell`). Dynamic completions (`CompleteEnv`) are still behind `unstable-dynamic`; skip.
- Use `clap`'s `ValueEnum` for `--format human|json` and `--color auto|always|never`. cargo-dist can ship the generated completion files in the archive if you generate them in a `build.rs` with `generate_to`, but the subcommand approach is simpler and is what most tools do.

---

## 5. Config

- **toml 1.1.5** (2026-09-02), **serde 1.0.229** (2026-07-18), **serde_json 1.0.151** (2026-07-20).
- toml 1.x split the parser into `toml_parser` / `toml_writer`; `toml_edit` is still the format-preserving editor but you do not need it for read-only config. `toml::from_str::<Config>(text)`; `toml::de::Error` has `span() -> Option<Range<usize>>` (byte range) and `message()`, and its `Display` already renders a rustc-style snippet with a caret, which is enough for the config-error path.
- `toml::Spanned<T>` (byte-range `span()`) as a field type gives you spans for semantic errors ("unknown language `pyton`") without a second parse.
- `#[serde(deny_unknown_fields)]` on every config struct; `#[serde(default)]` on optional sections; `#[serde(rename_all = "kebab-case")]` since TOML convention is kebab.
- Config discovery: hand-roll. `std::env::current_dir()?.ancestors().find(|d| d.join("anchr.toml").is_file())`. It is four lines; the crates that do this (`config`, `figment`) bring a lot of machinery you do not want. Stop at the first hit; also stop at a `.git` boundary if you want repo-scoped behaviour.

---

## 6. Diagnostics rendering

| Crate | Version | Last release | Notes |
|---|---|---|---|
| annotate-snippets | 0.12.16 | 2026-05-06 | rust-lang org, **this is rustc's renderer** since 2025 |
| codespan-reporting | 0.13.1 | 2025-10-22 | Stable, uses `termcolor` |
| ariadne | 0.6.0 | 2025-10-28 | Repo active (Mar 2026). Default spans are **char** offsets, byte variants exist |
| miette | 7.6.0 | 2025-04-27 | Repo quiet since Sep 2025 (only AGENTS.md commits in 2026). `fancy` pulls owo-colors, supports-color, textwrap, terminal_size, optional syntect |
| owo-colors | 4.4.0 | 2026-08-27 | |
| anstream | 1.0.0 | 2026-02-11 | Auto-degrades colour, respects `NO_COLOR`, `CLICOLOR_FORCE` |
| serde-sarif | 0.8.0 | 2025-05-09 | SARIF 2.1.0, typed builders |

**Fit for "grouped by cause, N locations":** annotate-snippets 0.12 is built around exactly this shape: a `Group` is one titled message with multiple `Snippet`s, each with its own `.path()` and its own `AnnotationKind::Primary/Context.span(byte_range).label(..)`. Spans are byte ranges. Colour goes through `anstyle`, so pair it with `anstream::println!` for TTY detection. It is the same look as rustc, which your users already read fluently.

```rust
use annotate_snippets::{AnnotationKind, Level, Renderer, Snippet};
let report = &[Level::ERROR
    .primary_title("unresolved reference `@ref[auth.session]`")
    .element(Snippet::source(md_src).path("docs/auth.md").line_start(1)
        .annotation(AnnotationKind::Primary.span(120..138).label("no anchor with this id")))
    .element(Snippet::source(rs_src).path("src/auth.rs").line_start(1)
        .annotation(AnnotationKind::Context.span(40..62).label("did you mean `auth.sessions`?")))];
anstream::eprintln!("{}", Renderer::styled().render(report));
```

**Recommendation:** annotate-snippets for human output; do not roll your own, the multi-file gutter alignment is more work than it looks. codespan-reporting is the fallback if you dislike the 0.12 builder API (it is also byte-based and multi-file). Skip miette: it wants to own your error types and its `fancy` footprint is large for a CLI that mostly emits JSON in CI. Skip ariadne because of the char-offset default.

**JSON:** define your own `#[derive(Serialize)]` types: `Report { version, summary: { errors, warnings }, diagnostics: [ { code: "unresolved-ref", severity, message, locations: [ { path, range: { start: {line, col, byte}, end: {...} } } ], suggestions: [..] } ] }`. Include both 1-based line/col and byte offsets so editors and scripts both work. Keep this your stable contract and derive human output from the same struct.

**SARIF:** worth adding as `--format sarif` later for GitHub code scanning. `serde-sarif` gives typed builders (`SarifBuilder`, `RunBuilder`, `ResultBuilder`, `LocationBuilder`, `PhysicalLocationBuilder`, `RegionBuilder`) generated from the 2.1.0 schema. Only ~10% more work once the JSON model exists. Fine to defer.

---

## 7. Error handling

- **thiserror 2.0.20** (2026-08-08), **anyhow 1.0.104** (2026-07-18).
- Convention holds: `thiserror` enums in the library crate(s) (`LexError`, `ParseError`, `ResolveError`, `ConfigError`) so the LSP and tests can match on them; `anyhow` only in `main.rs` for context wrapping and exit codes. thiserror 2 supports `#[error(transparent)]`, `#[from]`, and `#[source]` as before; the 2.0 change was mostly `no_std` and format-arg handling.
- Design note: user-facing diagnostics (unresolved refs) are not errors, they are data. Keep them out of `Result` entirely and reserve errors for "could not read file", "config invalid", "grammar failed to load".

---

## 8. Fuzzy "did you mean"

- **strsim 0.11.1** (2024-04-02). Stale date but it is a finished library with 1B downloads; nothing to fix. `edit-distance 2.2.2` (2025-10) is Levenshtein-only.
- **clap** uses `strsim::jaro` with a `> 0.7` confidence threshold and returns all candidates above it, sorted ascending. The code comment says they avoid `jaro_winkler` because an old strsim bug over-weighted long common prefixes.
- **rustc** uses restricted Damerau-Levenshtein (OSA distance) with a limit of `max(lookup.len(), 3) / 3`, tries case-insensitive exact match first, then edit distance, then a sorted-words fallback (split on `_`, sort, compare).
- **Recommendation:** rustc's approach fits identifiers better than Jaro. Anchor ids are short dotted/kebab tokens where a transposition or one-char typo is the common error, and a hard distance cap avoids absurd suggestions. Use `strsim::osa_distance` (that is the restricted Damerau-Levenshtein) with `limit = max(len, 3) / 3`, compare case-insensitively first, and additionally try matching on the last dotted segment (`auth.session` vs `auth.sessions`). Cap at 3 suggestions. Do not bother with a fuzzy-search crate; the candidate set is your anchor index, which is small.

---

## 9. LSP

| Crate | Version | Last release | Health |
|---|---|---|---|
| tower-lsp | 0.20.0 | 2023-08-11 | Effectively dead (last commit Jan 2024) |
| tower-lsp-server | 0.23.0 | 2025-12-07 | Community fork, repo active (commits 2026-08-31), MSRV 1.85, edition 2024 |
| async-lsp | 0.2.4 | 2026-04-24 | Active, oxalica; pins `lsp-types ^0.95` |
| lsp-server | 0.10.0 | 2026-07-16 | rust-analyzer's, sync, 5 deps, no lsp-types dep |
| lsp-types | 0.97.0 | 2024-06-04 | **Unmaintained** (no commits since) |
| ls-types | 0.0.6 | 2026-03-08 | tower-lsp-community fork of lsp-types, LSP 3.17 + `proposed` 3.18 |

**Recommendation for your scope (diagnostics, goto-def, references, rename, document symbols): `lsp-server` + `ls-types`.**

- It is synchronous: `Connection::stdio()`, `connection.initialize(server_capabilities_json)?`, then `for msg in &connection.receiver { match msg { Message::Request(r) => ..., Message::Notification(n) => ..., Message::Response(_) => ... } }`. Your server is a pure function of "current file set → index → diagnostics"; there is no I/O concurrency to justify tokio, and a sync loop keeps `anchr lsp` from dragging tokio into a binary whose main job is a batch check. Handle `didChange` by re-indexing the changed document (your parse is milliseconds), publish diagnostics, done.
- `lsp-server` defines only the JSON-RPC envelope, so you choose the types crate. Pick **`ls-types`** over `lsp-types 0.97`: same API surface (it is a fork), but maintained, and its `Uri` is the newer `fluent-uri`-based type (no `url` crate). Note it is 0.0.x, so pin exactly and expect churn.
- If you later want middleware (cancellation, concurrency limits), **`async-lsp`** is the better tower-based option: `&mut self` handlers, both server and client roles, well-designed. But it pins `lsp-types 0.95`, which is a slightly older protocol snapshot.
- `tower-lsp-server` is fine too and ergonomic (`#[tower_lsp_server::async_trait] impl LanguageServer for Backend`), but it forces tokio + `&self` handlers with async locks around your index. Not needed here.
- Position mapping: LSP positions are UTF-16 code units by default; negotiate `positionEncoding: utf-8` in `initialize` (3.17) and fall back to UTF-16 via `line-index` (see 15).

---

## 10. Paths

- **camino 1.2.5** (2026-07-28). Recommended: everything you print, put in JSON, or put in an LSP URI has to be a `str` anyway, and Markdown files are not going to have non-UTF-8 names. Convert once at the walker boundary with `Utf8PathBuf::try_from(entry.into_path())` and treat failure as a skipped-file warning. Enable the `serde1` feature.
- **normpath 1.5.1** (2026-05-05): `PathExt::normalize()` does hit the filesystem (it resolves as far as the path exists); `normalize_virtually()` is Windows-only. **path-clean 1.0.1** (2023, tiny, pure-lexical `..` collapsing). **dunce 1.0.5** (2024-08, strips `\\?\` UNC prefixes on Windows; use `dunce::simplified(&canonical_root)` once on the root).
- **Path containment without following symlinks:** hand-roll, do not use `canonicalize`. Lexically normalize the candidate relative to the root:

```rust
fn resolve_within_root(root: &Utf8Path, candidate: &Utf8Path) -> Result<Utf8PathBuf, PathEscapesRoot> {
    let mut depth = 0usize;
    let mut out = root.to_path_buf();
    for component in candidate.components() {
        match component {
            Utf8Component::Normal(segment) => { out.push(segment); depth += 1; }
            Utf8Component::CurDir => {}
            Utf8Component::ParentDir => {
                if depth == 0 { return Err(PathEscapesRoot); }
                out.pop(); depth -= 1;
            }
            Utf8Component::RootDir | Utf8Component::Prefix(_) => return Err(PathEscapesRoot),
        }
    }
    Ok(out)
}
```

`components()` already drops repeated separators and interior `.`. Symlinks inside the repo pointing outside are a separate policy (the `ignore` walker does not follow links by default, so you inherit the safe choice). This removes the need for path-clean entirely.

---

## 11. Index persistence

- **serde_json 1.0.151**, **postcard 1.1.3** (2025-07, stable documented wire format, serde-based, not self-describing), **blake3 1.8.7** (2026-08-20), **xxhash-rust 0.8.18** (2026-07-21).
- **bincode: do not use.** 3.0.0 (Dec 2025) is a tombstone release; the README says development has ceased after a harassment incident and the crate is unmaintained. Alternatives it names: `wincode`, `postcard`, `rkyv`.
- **Recommendation: start with no cache.** Parsing Markdown and running tree-sitter over a repo is fast enough that a cache is premature; ripgrep-style parallel walking will finish a 10k-file repo in well under a second on warm disk. When you do add one (for the LSP or huge monorepos), use **JSON via serde** keyed by `(path, size, mtime)` with a `blake3` content hash as a tiebreaker, stored under `.anchr/cache.json` and gitignored. JSON is debuggable, schema evolution is forgiving (`#[serde(default)]`), and the extra parse cost is irrelevant at this size. Add a `version` field and discard on mismatch. `xxhash-rust` (xxh3) is faster than blake3 but blake3 is already fast, has SIMD, and gives you a collision story you never have to think about.

---

## 12. Parallelism

- **rayon 1.12.0** (2026-04-14).
- `ignore`'s parallel walker already gives you a thread per core with per-thread state, which is exactly the shape you need for tree-sitter parsers. Collect results through a `crossbeam-channel` or `std::sync::mpsc` sender cloned into each visitor. That covers indexing. Resolution and diagnostic sorting after the walk are cheap and single-threaded is fine. **Skip rayon** unless profiling shows the post-walk phase matters; it is easy to add later (`par_iter()` on the file list).

---

## 13. Testing

| Crate | Version | Last release |
|---|---|---|
| insta | 1.48.0 | 2026-06-11 |
| assert_cmd | 2.2.2 | 2026-05-11 |
| predicates | 3.1.4 | 2026-02-11 |
| tempfile | 3.27.0 | 2026-03-11 |
| proptest | 1.11.0 | 2026-03-24 |

- insta: `assert_snapshot!` for human output, `assert_json_snapshot!` (feature `json`) for the JSON report, `with_settings!({ filters => vec![(r"/tmp/\S+", "[TMP]")] }, { .. })` to scrub temp paths, `glob!("fixtures/*.md", |path| ...)` for a fixture corpus. Use `cargo insta review`.
- assert_cmd: `Command::cargo_bin("anchr")?.current_dir(tmp.path()).args(["check", "--format", "json"]).assert().code(1).stdout(predicates::str::contains("unresolved-ref"))`. Build fixture repos in `tempfile::tempdir()`; run `git init` only in tests that exercise gitignore behaviour (or rely on `require_git(false)`).
- proptest for the lexer: generate arbitrary strings plus injected valid markers, assert (a) never panics, (b) every returned span slices to a string that re-lexes to the same token, (c) offsets are monotonic. Also a regression corpus for CRLF and multi-byte text before markers.

---

## 14. Security / supply chain / release

- **cargo-deny 0.20.2** (2026-07-09), **cargo-audit 0.22.2** (2026-06-05), **cargo-vet 0.10.2** (2026-01-13).
- Use `cargo-deny` in CI (advisories + licenses + bans + sources in one tool; it subsumes cargo-audit's check). cargo-vet is heavier process (audit records per crate); adopt only if you want the Mozilla-style trust model. `#![forbid(unsafe_code)]` is feasible in every crate of yours (see 2). Add `cargo deny check bans` with `multiple-versions = "deny"` early so the `lsp-types`/`ls-types` and `tree-sitter` version stories stay clean.
- **cargo-dist 0.32.0** (2026-05-22). The tool is now called `dist` (binary `dist`, crate still `cargo-dist`), maintained by axodotdev with Astral's fork changes merged upstream in 0.29; releases continue in 2026. Supports `installers = ["shell", "powershell", "npm", "homebrew", "msi"]` and `targets` including `x86_64-unknown-linux-musl` and `aarch64-unknown-linux-musl` for fully static Linux binaries (Windows and macOS binaries are static-enough by default).
- **npm layout: it does NOT use the per-platform `optionalDependencies` pattern.** cargo-dist publishes one npm package (`@scope/anchr` via `npm-scope` / `npm-package`) containing a JS shim that downloads the matching prebuilt archive from GitHub Releases at install time. 0.32 dropped axios/rimraf in favour of Node builtins (min Node 14.14). If you specifically want the esbuild/biome-style platform-package layout (works offline, no postinstall network), you would have to build that yourself; cargo-dist's `dist-manifest.json` gives you the artifact list to script it from.

---

## 15. Line/column mapping

- **line-index 0.1.2** (2024-10, rust-analyzer's, tiny, depends on `text-size` + `nohash-hasher`). `LineIndex::new(&text)`, `line_col(TextSize) -> LineCol` (0-based line, UTF-8 byte column), `offset(LineCol) -> Option<TextSize>`, `to_wide(WideEncoding::Utf16, line_col) -> Option<WideLineCol>` for LSP. Handles multi-byte UTF-8; treats `\n` as the line terminator so CRLF just leaves the `\r` in the previous line's column count, which is what LSP expects.
- **ropey** is at 1.6.1 stable / 2.0.0-beta.1 (2025-08) and is for editable text buffers; overkill unless the LSP does incremental edits (it will not need to, you re-parse whole documents).
- **Recommendation: line-index.** It is the exact problem, it has the UTF-16 conversion you need for LSP, and it is a couple hundred lines you would otherwise write. `text-size` uses `u32` offsets, which caps files at 4 GiB, fine. Build one index per file lazily and keep it alongside the source text in your document store. Hand-rolling a `Vec<u32>` of newline offsets plus `partition_point` is also fine if you want zero deps, but the UTF-16 path is where hand-rolled code gets buggy.

---

## Summary

| Area | Crate | Version | Reason |
|---|---|---|---|
| Markdown | pulldown-cmark | 0.13.4 | Byte-range `into_offset_iter`, distinct Text/Code/CodeBlock events, pull-based |
| Parsing runtime | tree-sitter | 0.27.0 | ABI 13–15 accepted; all grammars via `tree-sitter-language` 0.1 |
| Grammars | tree-sitter-{rust 0.24.2, typescript 0.23.2, javascript 0.25.0, python 0.25.0, go 0.25.0} | | Each exports `LANGUAGE` + `TAGS_QUERY`; static link, ~5 MB total |
| Declarations | run `TAGS_QUERY` via `Query`/`QueryCursor` | | Avoids tree-sitter-tags dep; same API as comment queries |
| Walking | ignore + globset | 0.4.33 / 0.4.20 | Parallel walk with per-thread state, gitignore, overrides |
| CLI | clap (derive) + clap_complete | 4.6.6 / 4.6.9 | Standard |
| Config | toml + serde | 1.1.5 / 1.0.229 | `Spanned<T>`, `de::Error::span()`, `deny_unknown_fields`; hand-roll ancestor search |
| Human diagnostics | annotate-snippets + anstream | 0.12.16 / 1.0.0 | rustc's renderer, multi-file groups, byte spans |
| JSON diagnostics | serde_json (own schema) | 1.0.151 | Stable contract; SARIF via serde-sarif 0.8.0 later |
| Errors | thiserror (lib) / anyhow (bin) | 2.0.20 / 1.0.104 | Convention |
| Suggestions | strsim `osa_distance` | 0.11.1 | rustc's algorithm and cap, fits identifiers |
| LSP | lsp-server + ls-types | 0.10.0 / 0.0.6 | Sync loop, no tokio, maintained types fork |
| Paths | camino (+ dunce on Windows) | 1.2.5 / 1.0.5 | UTF-8 everywhere; hand-rolled lexical containment |
| Cache | none now; serde_json + blake3 later | 1.0.151 / 1.8.7 | bincode is unmaintained |
| Parallelism | ignore's walker only | | rayon 1.12.0 only if profiling says so |
| Tests | insta, assert_cmd, predicates, tempfile, proptest | 1.48.0 / 2.2.2 / 3.1.4 / 3.27.0 / 1.11.0 | |
| Line/col | line-index | 0.1.2 | UTF-16 conversion for LSP built in |
| Supply chain | cargo-deny; `#![forbid(unsafe_code)]` | 0.20.2 | Our crates need no unsafe |
| Release | cargo-dist (`dist`) | 0.32.0 | musl targets; npm installer is a download shim, not optionalDependencies |

**Flags / not fully verified:**
- Compiled grammar sizes are estimates from `parser.c` byte counts and third-party `.so` sizes, not a measured build.
- `ignore` override-vs-gitignore precedence stated from memory of the `ignore` source; the ripgrep guide fetch was ambiguous. Confirm with a two-line test before relying on whitelist-un-ignores-gitignored behaviour.
- `pulldown_cmark::utils::TextMergeWithOffset` exists in 0.13 but its exact generic signature was not readable.
- `tree-sitter-tags 0.27.0` `Tag` struct field types were not readable on docs.rs; field names are from the crate summary.
- docs.rs showed drifted "release" dates for codespan-reporting and serde-sarif; the crates.io dates in the tables are authoritative.
