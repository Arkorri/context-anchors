# Review of `ignore-question-4-assume-soft-widget.md` (code-level design, v1)

Reviewed against: `DESIGN.md`, `DISTRIBUTION.md`, the crate survey, and the corgea security digest. Findings are ordered most severe first.

## Findings

**1. Blocker. §3.3 / §4 / §13. Include globs as `ignore` overrides un-ignore gitignored files.**
`ignore`'s `Ignore::matched` consults overrides first and returns immediately on any override match ("always overrides any other ignore logic" is the ripgrep man-page wording for `-g`). With one whitelist glob present, every file gets a Whitelist or Ignore verdict from overrides and `.gitignore` is never consulted for files (directories still prune, because the whitelist-forces-a-verdict rule applies only to non-dirs). The default config always sets `include`, so gitignored files such as `CHANGELOG.md` or `*.generated.ts` get scanned, contradicting "`exclude` is added to `.gitignore` semantics, never replaces them." The survey flagged this precedence as unverified; the plan says "we do not intend to rely on it" while relying on it by default.
Fix: use overrides only for `exclude` (`!` globs). Apply `include` as a `globset` post-filter inside the visitor (or via `TypesBuilder`, which `ignore` checks after ignore rules). Add the two-line precedence test the survey asked for.

**2. Blocker. §3.5 Symbol. A symbol not found in a tree that has parse errors must be Unverified, not an error.**
The plan says parse errors do not fail the file and the tree is error-tolerant. For comment extraction that is fine. For symbol lookup it produces a false `SymbolMissing` whenever the declaration sits inside an ERROR node, which the stale TypeScript grammar (late 2024) will do on newer syntax. That violates both "unverifiable never renders as valid" and "zero false positives".
Fix: found ⇒ Resolved; not found and `root_node().has_error()` ⇒ `Unverified::ParseErrors { path, language }` with a hint about grammar staleness; not found and clean tree ⇒ `SymbolMissing`. Add a fixture per language with deliberately broken syntax around a declaration.

**3. Blocker. §3.5 Path. `symlink_metadata` on a case-insensitive filesystem makes `check` nondeterministic across platforms.**
`@ref[src/Foo.ts]` resolves on macOS default APFS when the file is `foo.ts`, then fails in Linux CI. This breaks invariant 5 (deterministic) in the most common dev/CI split.
Fix: resolve by exact-name lookup against a `read_dir` of the parent, cached per directory for the run. The plan already reads the parent directory for sibling suggestions, so this is nearly free and shares one cache. Apply the same exact-name check per component for directories on the path.

**4. Should-fix. §3.1 / §3.3. Per-thread `Parser` ownership contradicts `Container::text_regions(&self, source: &str)`.**
The signature has no way to receive the walker thread's parser. Either every call constructs a parser (works, but then "each visitor owns its Parser" is false) or the signature is wrong. Symbol resolution needs a parser too and runs on a different thread.
Fix: introduce a per-thread `FileAnalyzer { parser: Parser, registry: &LanguageRegistry }` (one `Parser` suffices; `set_language` switches per file) with `fn scan(&mut self, path, source) -> FileScan` and `fn symbols(&mut self, language, source) -> SymbolTable`. `Container` stays a pure selector enum.

**5. Should-fix. §3.7 / §3.4. The human renderer needs source text and a `LineIndex` after the index is built, and the plan does not say where they come from.**
`Site` carries only a `ByteSpan`. `annotate-snippets` needs the file's source for the rendered snippet, and every site list needs line/col. `FileScan.line_index` is dropped when scans fold into `Index`.
Fix: `FileRecord` keeps `LineIndex` (cheap), and `Report` sites carry `LineCol` computed at diagnostic construction. The renderer re-reads only the one file per diagnostic it renders as a snippet, degrading to the `path:line:col` list if the read fails or the byte length changed since scan.

**6. Should-fix. §3.1 LanguageRegistry / §7 / §10. A `LazyLock` registry that compiles `Query` objects cannot satisfy `expect_used = deny`.**
`Query::new` returns `Result`; a `LazyLock` initializer has no error channel, so it must `expect`. Grammar/query mismatch is a legitimate runtime failure (ABI or query syntax drift).
Fix: `LanguageRegistry::new() -> Result<Self, RegistryError>`, built once in `run_check` and passed by `&`. One unit test asserts construction succeeds so drift is caught in CI.

**7. Should-fix. §10 `#[non_exhaustive]` on `DiagnosticKind`, `Unresolved`, `Unverified`.**
These enums are matched exhaustively in the binary crate (JSON and human renderers). `non_exhaustive` applies across crates, so it forces `_ =>` arms in the renderer and silently defeats the "add a variant, every match fails to compile" property the plan cites as a goal. `anchr-core` is not a published library in v1.
Fix: drop `non_exhaustive` from core enums. Keep it, if anywhere, only on JSON DTO enums that external consumers deserialize.

**8. Should-fix. §2 / §3.5. Cross-root prefix is allowed on all target kinds but resolution only defines root handling for anchors.**
`Path` and `Symbol` say `root.dir.join(path)` with no rule for `Some(root)` that is absent or undeclared.
Fix: factor root selection into one step before dispatch: `None` ⇒ current; undeclared ⇒ `RootUndeclared` (error); `Absent` ⇒ `RootAbsent` (unverified); else proceed with that root's dir. All three variants go through it.

**9. Should-fix. §2 / §3.5 Path. Trailing `/` is lost in `RelPath` normalization.**
`Utf8PathBuf` drops the trailing separator, yet resolution says a trailing `/` requires `is_dir()`. The `RefTarget::Path` variant has no field to carry it.
Fix: `Path { root, path, expects: PathExpectation::{Any, Directory} }`, set by `parse_target` before normalization. Enum, not bool, per the plan's own rule.

**10. Should-fix. §3.5 Symbol / §11 steps 9–10. `SymbolTable { names: HashSet<String> }` and whole-marker `Site.span` are too coarse for the LSP and rename.**
Go-to-definition on `file#Symbol` needs the declaration's span; rename of `claude:#auth/flow` must rewrite only the id bytes, not the marker. Both are trivial to record now and a refactor later.
Fix: `SymbolTable { declarations: HashMap<String, Vec<ByteSpan>> }`. `Marker` records `body_span` and, for anchor ids and `#id` targets, `id_span`.

**11. Should-fix. §3.3 / §3.4 / §4. External roots: what is scanned and what is reported is undefined.**
Scanning `~/.claude` fully on every check of every repo collects its refs too. Reporting its broken refs would be wrong (you are checking your repo), and its cross-root refs cannot resolve anyway since its `[roots]` table is ignored.
Fix: scan external roots in an `AnchorsOnly` mode (refs and malformed markers discarded). Report duplicate anchors in an external root as unverified, since you cannot fix them from here. State this in §3.5 and §4.

**12. Should-fix. §3.5 Symbol. `disable_pattern` on `reference.*` patterns is the wrong lever, and `TAGS_QUERY` coverage is assumed rather than planned for.**
There is no direct pattern-to-capture-names API; finding reference patterns means text-scanning `start_byte_for_pattern` ranges. The survey's per-match filter on the `definition.*` capture name is simpler. Separately, the plan's tests list "type alias" and "const", but TS's `tags.scm` is short and may not capture `type_alias_declaration`, `enum_declaration`, or `export const f = () => {}`, which is the dominant declaration form in TS codebases.
Fix: filter per match. Give `LanguageSpec` a `supplementary_declarations: &'static str` appended to `TAGS_QUERY`, and write the per-kind fixture list before implementing so gaps surface as failing tests rather than false `SymbolMissing` errors.

**13. Should-fix. §2 target grammar. `Foo::bar` and `Class.method` symbol paths are accepted by the grammar and will never match.**
`symbol_name := [^\s\]]+` accepts `Foo::bar`; the tags `@name` capture is `bar`. Users will write qualified names constantly. Silent guaranteed failure with a "did you mean" that cannot help.
Fix: decide now. Either reject `::` and `.` in symbol names with a `TargetError::QualifiedSymbol` message explaining the file-scoped guarantee, or match on the last segment and document it. Rejecting is recommended: it matches the stated guarantee and keeps the error honest.

**14. Should-fix. §2 `RelPath` / §10 item 14. `rel_path` is denylist-validated; the plan claims allowlist parsing for all newtypes.**
`AnchorId`, `RootName`, `SymbolName` are allowlists. `RelPath` only rejects `""`, leading `/`, `..`, `\`. So `@ref[[x]`, `@ref[ docs ]`, and paths with control characters pass parsing and become `PathMissing` errors on odd prose.
Fix: allowlist the segment charset (printable, non-whitespace, excluding `[ ] # : \` and control chars), reject a bare `.` after normalization, and add these to the `parse_target` rejection table.

**15. Should-fix. §3.6 grouping. `Malformed(MalformedMarker)` embeds the `Site` in the grouping key, and file-level diagnostics have no meaningful `Site`.**
Every malformed marker becomes its own group with `sites` duplicating the key. `FileNotUtf8` / `FileTooLarge` have no marker span.
Fix: `DiagnosticKind::Malformed { kind: MarkerKind, reason: MalformedReason, raw: String }` with the site in `Diagnostic.sites`. Make the location an enum: `Sites(Vec<Site>)` or `Files(Vec<(RootName, RelPath)>)`.

**16. Should-fix. §3.3 / §3.5. Skipped files interact with `AnchorMissing`, and the 100k symbol cap silently truncates.**
An `@anchor[x]` in a 3 MiB markdown file is never indexed, so `@ref[#x]` elsewhere becomes a hard error. The `SymbolTable` cap produces a false `SymbolMissing` for the 100,001st declaration.
Fix: when the symbol cap is hit, return `Unverified::SymbolTableTruncated`, never a truncated table. For skipped files, word the `FileTooLarge` diagnostic to say anchors in it are unindexed, and document that `max-file-bytes` is the knob. Both follow from "unverifiable never renders as valid."

**17. Should-fix. §3.2 lexer / §2. Preceding-byte post-filter is ASCII-only, and the grammar's `:` justification is false.**
`é@ref[x]` passes (the preceding byte is a UTF-8 continuation byte, not ASCII alphanumeric) while `e@ref[x]` is rejected. Separately, `:` is legal in filenames on Linux and macOS, so "a path can legally contain `:` on no platform we support" is wrong; `@ref[foo:bar.md]` parses as root `foo` and errors as `RootUndeclared`.
Fix: check the preceding `char` via `source[..start].chars().next_back()`. Keep the lexical root rule, but state it as a reservation (`:` after a root-shaped prefix, `#` anywhere) in the grammar docs rather than justifying it by platform.

**18. Should-fix. §9 testing. Missing cases for the failure modes the design cares most about.**
Add: CRLF files and `\r` inside a body; multi-byte text before a marker (offset and line/col); escaped `@ref\[x\]` in markdown is not lexed (the source-slice approach makes this a free escape hatch, worth documenting); fenced block inside blockquote and list item, tilde fence, longer closing fence, unclosed fence at EOF, indented code inside a nested list (the complement approach's only failure mode is an imprecise code range letting an example through); undeclared root ⇒ error versus absent ⇒ unverified; case-mismatched path; parse-error tree ⇒ unverified (finding 2); a human-output snapshot with color forced off; `PATHS` filtering semantics; a bad-config exit-2 snapshot with the caret rendering.

**19. Should-fix. §5 `init`. Under-specified for a command that writes files.**
It writes `anchr.toml`, instructions, and hook config. Hook config for Claude Code means merging into `.claude/settings.json`, a JSON document with existing content. No overwrite policy, no dry run, no idempotency rule.
Fix: never overwrite an existing file without `--force`; settings merge via `serde_json::Value` read-modify-write that preserves unknown keys; print every path written; `--dry-run`. Or defer hook writing to printing instructions, which is what `DISTRIBUTION.md` §5 implies for non-Claude vendors anyway.

**20. Nit. §3.5 / §6 / §8. `DashMap` and the parallelism story contradict each other.**
§3.3 says the post-walk phase is single-threaded; §8 says the symbol cache is the only shared mutable state, implying parallel resolution. Single-threaded resolution needs a `HashMap`.
Fix: single-threaded resolution, `HashMap`, drop `dashmap`. Parallelize later if profiling says so.

**21. Nit. §10 lints. `missing_docs = "warn"` under `-D warnings` forces a doc comment on every `pub` item.**
That conflicts with the governing "default to no comment" principle and with the plan's own stance. Drop it or scope it to the crate root.

**22. Nit. §10 claims stated as structural that are not.**
"Output never contains file content" is false for the human renderer (snippets are file lines). "Every config file is untrusted" is aspirational: a hostile `anchr.toml` can declare `roots.home = "~"` and the tool will walk it. `max-file-bytes` is user-settable above `u32::MAX`, which `text-size` cannot represent. There is no parse deadline on tree-sitter for pathological inputs.
Fix: reword the first two honestly (config is parsed defensively, its declared roots are trusted). Validate `max-file-bytes <= u32::MAX` after deserialization. Use `parse_with_options` with a progress callback that aborts after a budget, reported as `Unverified::ParseTimeout`.

**23. Nit. §5 `--strict`. The name promises more than "absent root is an error."**
CI users will expect `--strict` to fail on any unverified. Either name it `--absent-root=error` or define `--strict` as "all unverified become errors" and make the config field match.

**24. Nit. §2 / §3.4. The current root has no name, and `Index` holds both `refs` and `files`.**
`Site.root: RootName` needs a value for the current root; config has no name field. `refs: Vec<...>` and `files: HashMap<RelPath, FileRecord>` overlap; `update_file` must keep both in sync.
Fix: optional `[root] name`, defaulting to the directory name, validated as a `RootName`. Make `files` the single source and derive the `anchors` map; `backrefs` iterates `files`.

**25. Nit. §10 panics policy. Verify `ignore`'s parallel walker re-raises a visitor panic, and note the LSP consequence.**
The claim "propagates when the walk joins" is plausible but unverified. For step 9, a panic on one document kills the server, so the LSP loop will need `catch_unwind` per request even if the CLI does not. Say so now so the policy is not contradicted later.

## Readiness

Not ready as written. Findings 1 through 3 each produce false errors or platform-dependent results, which is the one thing the design says kills adoption; they are small changes but must land in the plan before step 3 (walker and text regions) and step 5 (resolution). Findings 4 through 10 and 13 through 15 are shape decisions that steps 2 and 5 bake in, so fix them before step 2 starts; each is a paragraph. After those, milestone 1 can proceed and the remaining items can be folded in during steps 6 through 9. The crate choices hold up, the exclusion-complement approach is sound, and the module split is right; the gaps are in resolution semantics and the lifecycle of per-file data, not in the architecture.
