# Alias imports — file-scoped names for references

**Status:** implemented; §7 lists the stages as they were built. Companion to
@ref[DESIGN.md] (the guarantee) and @ref[CODE_DESIGN.md] (the pipeline this extends).
<!-- refs -->
@ref[crates/anchr-core/src/text/mod.rs#FileAnalyzer as FileAnalyzer]
@ref[crates/anchr-core/src/resolve/mod.rs#Unresolved as Unresolved]
@ref[#cli/check as Check]
@ref[#cli/coverage as Coverage]
@ref[#cli/backrefs as Backrefs]
@ref[#cli/annotate as Annotate]
@ref[#cli/init as Init]
@noref[src/as/x.rs, docs/x.md, alias_uses, Alias]

## 1. Problem

The dogfood pass that put 79 live references into this repository's design documents exposed the
authoring cost of the grammar. A qualified symbol reference is about fifty characters:

```text
@ref[crates/anchr-core/src/text/mod.rs#FileAnalyzer]
```

A document that mentions @[FileAnalyzer] ten times will get one such reference and nine backticked
mentions. Those nine are exactly the rot the tool exists to catch, and today neither @[Check] nor
@[Coverage] can see them: @[Check] only knows about markers, and @[Coverage] classifies a
backticked identifier that resolves nowhere as "ignored", because most backticked words are
`HashMap`.

The convention adopted in the dogfood pass, "annotate the first mention per section", papers over
this and fails the one test an authoring rule has to pass: the writer cannot decide from the
sentence alone. To add a sentence about @[FileAnalyzer], they must know whether the section already
carries a reference, which means reading the section, knowing that the granularity is "section",
and knowing what counts as one. An agent has to grep first. Every step is a place to be wrong, and
being wrong is silent.

Two concrete failures from that pass:

- @ref[crates/anchr-core/src/root.rs#FilePath] appeared twice in the implementation notes and was
  never annotated. The coverage report listed it among 154 candidates that were deliberate skips,
  and nobody could tell the miss from the skips.
- Eight mentions of @[Init]: the tool proposed a symbol reference to a function that happens to
  share the name, when the prose meant the CLI subcommand. Under a first-mention rule the
  annotated mention passes for the wrong reason and the other seven are unchecked.

## 2. Precedent

Programming languages solved this problem a long time ago: **import once, use by a local name**.
The construct is `import x as y`, and its semantics are borrowed whole rather than reinvented.

| Language rule | anchr rule |
|---|---|
| An import binds a local name to an external definition | A declaration binds an alias to a target |
| Scope is the module | Scope is the file |
| Use of an undeclared identifier is an error | A use with no declaration in the file is an error |
| Duplicate declaration in one scope is an error | Two declarations of one alias in a file is an error |
| A broken import errors at the import, not at every use | The declaration is resolved; uses bind to it and are not re-resolved |
| Unused import is a lint, not an error | Unused alias is reported by @[Coverage], never by @[Check] |
| Style guides put imports at the top; the grammar allows them anywhere | Declarations anywhere in the file; the @[Init] template teaches an index block at the top |
| Name resolution is a semantic pass after lexing | Binding happens in the index, not the lexer |

Existing anchr precedent points the same way. `@anchor` already decouples an identity from the
heading text that carries it; an alias decouples the document's vocabulary from the code's
spelling in the same manner. The `root:` prefix already follows the module-resolution analogy
("bare is local, prefixed is external").

What this buys:

- **Every mention is checked.** A stale mention is a failing marker, not backticked text.
- **A rename in code is one edit per file.** The prose keeps saying `@[Analyzer]`; only the
  declaration line changes. That is better than annotating every mention with the qualified
  form, where a rename touches N sites.
- **Ambiguity is dissolved, not detected.** Two `Auth` types become `UserAuth` and `AdminAuth` by
  the author's choice at declaration. No uniqueness heuristics, no "declared in 2 files" error to
  interpret.
- **The authoring rule is local.** "Am I using a name this file imported?" is answerable from the
  file alone.

## 3. Grammar
<!-- @anchor[aliases/grammar] -->

The grammar in @ref[#design/grammar] gains one production and one optional clause:

```text
marker      := anchor | ref | use
anchor      := "@anchor[" anchor_id "]"                       unchanged
ref         := "@ref[" target [ ws "as" ws alias ] "]"       alias clause is new
use         := "@[" alias "]"                                 new
alias       := [A-Za-z_] [A-Za-z0-9_]*                        max 64 bytes, case-sensitive
ws          := one or more spaces or tabs
```

```markdown
@ref[crates/anchr-core/src/text/mod.rs#FileAnalyzer as Analyzer]
@ref[#design/architecture as Architecture]

Each walker thread owns an @[Analyzer]; the resolver has its own @[Analyzer].
```

Each rule, with its reason:

**Brackets on the use site.** `@[` is the existing marker opener with an empty kind, so the lexer
in @ref[crates/anchr-core/src/marker/lex.rs] stays one regex:

```text
@(anchor|ref|)\[(?:([^\[\]\n]*)\]|)
```

Leftmost-first alternation still rejects `@reference[`. The alternative, a bare `@Alias` sigil,
was rejected because source-comment regions carry JSDoc and Javadoc tags (`@param`, `@returns`)
and prose carries `@handle` mentions; with undeclared-is-error semantics every one of them would
be an error, and without those semantics a typo is silently plain text. The preceding-character
rule and both escape forms (`\@[X]`, backticks, fences) apply unchanged; `user@[1.2.3.4]` is
glued and never a marker. The one known real-world collision is Objective-C array literals
`@[a, b]` in prose, which become malformed-alias errors; `\@[` is the escape.

**`as` is the only keyword and whitespace is the only delimiter.** Targets and aliases cannot
contain whitespace, so a body containing whitespace is an alias clause or malformed. Leading or
trailing whitespace is always malformed, so `@ref[ x ]` stays an error as today. Otherwise the
body is split on whitespace (spaces, tabs, a stray `\r`): one token is a plain reference, exactly
three tokens with the middle one the literal `as` is a declaration, anything else is one error,
`BadAliasClause`, whose message reads "a target has no spaces; to declare an alias write
`target as Alias`". Someone who typed `@ref[my file.md]` is then not sent chasing aliases. `as`
is case-sensitive; `as` as an alias *name* is legal and harmless. `as` as a path segment
(`src/as/x.rs`) is untouched because it contains no whitespace.

**Alias charset is ASCII identifier.** The same allowlist posture as every other newtype in the
grammar (@ref[CODE_DESIGN.md] §10). No `/`, `.`, or `-`: an alias is a name, not a path or an id,
and disjoint charsets mean an alias can never be mistaken for a target in a diagnostic. The
newtype mirrors @ref[crates/anchr-core/src/marker/symbol.rs#SymbolName].

**No reserved aliases.** `@anchor[` and `@ref[` do not overlap with `@[`, so `@[anchor]` is a
legal, if odd, alias use.

**Malformed shapes**, all with a `Use` marker kind: `@[]` is an empty body; `@[ X ]` is an
invalid alias (no trimming, the form is meant to be typed exactly); `@[X` with no `]` on the
line is unclosed; `@[@[X]]` is an unclosed opener followed by a use, mirroring today's
`@ref[@ref[x]]`.

**Anchors are never aliased at declaration.** `@anchor[x as Y]` is an invalid anchor id, as
today, because spaces are not id characters. An anchor *target* may be aliased:
`@ref[#design/scope as Scope]`.

**`@[target]` is not a shorthand for `@ref[target]`.** One form, one meaning. A use body is only
ever parsed as an alias.

**Spans.** A declaration records the alias token's byte span alongside the existing `id_span`.
The anchor `id_span` ends at the end of the target token, not at the end of the body; otherwise
`anchr rename` on `@ref[#x as Y]` would rewrite ` as Y` too.

## 4. Semantics
<!-- @anchor[aliases/semantics] -->

**Scope.** The alias table is per file and built from that file's markers alone, inside
@ref[crates/anchr-core/src/index.rs#FileRecord]. Declarations may appear anywhere in the file.
The same alias in two files is two unrelated bindings. There is no inheritance across files and no
root-level table (§8).

**Declaration.** `@ref[target as Alias]` is an ordinary reference in every existing sense: it is
resolved by @ref[crates/anchr-core/src/resolve/mod.rs#Resolver], counted in `refs_checked`, a
reference site for @[Backrefs], and its `id_span` is rewritten by `anchr rename` when the target is
an anchor. It additionally records the alias and its span.

**Use.** `@[Alias]` binds to the file's declaration of `Alias`. It is not re-resolved: the
declaration is the file's one check of the target, and a broken target reports at the
declaration line only. One site, one edit. A use whose alias has no declaration in the file is
`AliasUndeclared`, an error, with a did-you-mean drawn from the file's declared aliases. Bound
uses are counted separately in the summary as `alias_uses`; unbound ones are already in `errors`,
so `refs_checked + alias_uses` is exactly the number of checked mentions. Uses are included in
`backrefs(target)`, so find-references shows the declaration and every use.

**Duplicates.** Two declarations of one alias in a file is `AliasDuplicate`, an error listing every
declaration site, even when both name the same target (rustc's E0252 makes the same call). Uses
of a duplicated alias bind to the *first* declaration in file order for navigation and @[Backrefs]
and raise no diagnostic of their own: the import lines are the cause and the only fix sites.

**Malformed declaration.** `@ref[bad path as X]` is one malformed-marker diagnostic and registers
no alias, so every `@[X]` in the file also reports `AliasUndeclared`. Accepted for now. The
mitigation, if it bites, is letting a malformed marker still register its alias name.

**A broken declaration keeps its uses visible.** When an aliased declaration is unresolved or
unverified, the diagnostic carries the hint "alias `X` has N uses in this file" as a footer, not
as extra locations, and the language server attaches the use sites as related information.
Grouping is unchanged.

**Unused.** A declared alias with zero uses is reported by `anchr coverage`
(@ref[crates/anchr-core/src/coverage.rs]) as an advisory line and JSON candidate kind
`unused-alias`. @[Check] never reports it: check's contract is soundness of what is asserted, and
an unused import asserts nothing false.

**Diagnostics are keyed by file.** `AliasUndeclared` and `AliasDuplicate` carry the file path in
their @ref[crates/anchr-core/src/diagnostic.rs#DiagnosticKind] key, because the file *is* the
scope: "alias `Analyser` is not declared in `docs/x.md`" is the cause, and the did-you-mean
candidates differ per file. This is the same reasoning that put the path in `NoGrammar`. They are
siblings of `DuplicateAnchor`, not @[Unresolved] variants: @[Unresolved] is the resolver's
output,
and the resolver has no file context.

**External roots** are scanned anchors-only and drop all references today; declarations and uses
are references and are dropped too. **`PATHS` filtering** applies to undeclared and duplicate
findings by file, exactly as it does to references and malformed markers.

**Rename.** `anchr rename old new` (anchor ids, @ref[crates/anchr-core/src/rename.rs]) rewrites the
`id_span` of aliased declarations as for plain references; uses carry no id and are untouched.
Renaming an *alias* is a file-local operation, declaration token plus every use, exposed through
the language server's rename request, which disambiguates by cursor position: on the anchor id it
renames the anchor, on the alias token or a use it renames the alias. A CLI form is deferred.

**Invariants** (@ref[#design/invariants]) hold: deterministic, because binding is a pure function
of one file's bytes; opt-in, because nothing is a use unless written `@[...]`; unverifiable never
renders valid, because a use never claims resolution on its own, the declaration does or does not;
group by cause, because undeclared and duplicate group per file and alias
(@ref[#design/diagnostics]).

## 5. Worked example

```markdown
<!-- refs -->
@ref[crates/anchr-core/src/text/mod.rs#FileAnalyzer as Analyzer]
@ref[crates/anchr-core/src/root.rs#FilePath as FilePath]
@ref[#design/architecture as Architecture]

Each walker thread owns an @[Analyzer]; the resolver has its own @[Analyzer].
@[Architecture] says the index is never authoritative, so results are keyed by @[FilePath]
and rebuilt per run.
```

After @[FileAnalyzer] is renamed to `Analyzer` in code:

```text
error: no declaration named `FileAnalyzer` in `crates/anchr-core/src/text/mod.rs` (root `repo`)
 --> docs/internals.md:2:1
  = help: did you mean `Analyzer`?
  = note: alias `Analyzer` has 2 uses in this file
```

One site, one edit; the uses keep reading `@[Analyzer]`. A typo at a use:

```text
error: alias `Analyser` is not declared in `docs/internals.md`
 --> docs/internals.md:6:30
  = help: did you mean `Analyzer`?
```

## 6. Authoring conventions
<!-- @anchor[aliases/conventions] -->

Best practice, not grammar. The @[Init] template (@ref[crates/context-anchors/templates/ANCHR.md])
teaches these.

- **Declarations go in an index block at the top of the file**, under an HTML comment
  (`<!-- refs -->`), the way imports sit at the top of a module. The grammar allows them anywhere,
  as most languages do, and every style guide then puts them at the top anyway.
- **Alias the document's name for the thing, not the code's** when the two differ (`Analyzer`
  rather than @[FileAnalyzer]). A rename in code then touches one line.
- **Concepts without a declaration get an anchor.** A CLI subcommand or a section is not a symbol;
  put `@anchor[cli/init]` where it is defined and alias the anchor reference, rather than pointing
  a symbol reference at a function that happens to share the name.
- **One alias per thing per file.** If a file needs two `Auth`s, it names them `UserAuth` and
  `AdminAuth`. That is the ambiguity being dissolved, not worked around.

## 7. Pipeline changes

Names refer to @ref[crates/anchr-core/src/] unless noted; each item is one PR in the stack.

1. **Grammar and lexing.** @ref[crates/anchr-core/src/marker/alias.rs] (new):
   @ref[crates/anchr-core/src/marker/alias.rs#Alias] newtype with allowlist and limits.
   @ref[crates/anchr-core/src/marker/target.rs#parse_target] tokenises the body first, runs the
   existing grammar on the target token, and shifts spans by the token's offset. The regex above;
   `MarkerKind::Use`, `MarkerPayload::Use`, an alias on `MarkerPayload::Ref`; a malformed reason
   for invalid aliases. Existing exhaustive matches learn to ignore uses; fuzz targets `lex` and
   `parse-target` cover the new shapes. Green on its own: no `@[` exists in scanned files today.
2. **Binding and diagnostics.** The per-file alias table in
   @ref[crates/anchr-core/src/index.rs#FileRecord], built when a file is indexed and looked up
   lazily so the table is the single owner. @[Backrefs] chains direct references with bound uses;
   a reference site records whether it came through an alias so @ref[#cli/rename] skips uses
   explicitly. `AliasUndeclared` and `AliasDuplicate`; the summary's
   `alias_uses`; JSON codes `alias-undeclared` and `alias-duplicate` (schema stays 1, the change
   is additive); the human summary line.
3. **@[Coverage].** Per-file, case-sensitive matches against that file's aliases: an inline code span
   whose whole content is an alias (high confidence) and a bare word outside links (lower; an alias
   like `Scope` matches English) become `@[X]` proposals that @[Annotate] applies. Unused aliases
   are advisories, excluded from the proposal list and from the total, since they are not
   reference-shaped strings. Bound uses count as annotated.
4. **Language server** (@ref[crates/context-anchors/src/lsp/server.rs]). Definition on a use
   returns the target's locations plus the declaration; references synthesize the target from the
   binding; rename disambiguates by cursor offset; document symbols list declarations.
5. **Documentation and dogfood.** @ref[#design/grammar], the README, the @[Init] template, and the
   code-level design are updated; this repository's own documents convert their repeated mentions
   to aliases with an index block per file, and the before/after coverage numbers are recorded.

### Security posture

An alias is an allowlisted, bounded newtype (@ref[CODE_DESIGN.md] §10, input bounds). The per-file
table is bounded by the marker count, itself bounded by `max-file-bytes`. Suggestions run over one
file's aliases. No new filesystem reads. The lexer stays a single linear-time regex.

## 8. Deferred, with reasoning
<!-- @anchor[aliases/deferred] -->

- **Root-level alias table** (`[aliases]` in `anchr.toml`). Removes per-file redeclaration but makes
  a use depend on state outside the file, the property file scope buys. Revisit if redeclaration
  measurably hurts.
- **A "not a reference" marker** for acknowledging coverage candidates. Shipped since, as `@noref`
  plus `[coverage] ignore`, once the annotated repository showed that most remaining candidates
  were correctly classified and still not references: @ref[docs/design/ignores.md].
- **Smarter @[Annotate].** Propose a declaration plus uses for repeated identifiers instead of N
  qualified references. Needs aliases to exist first.
- **CLI alias rename.** `anchr rename` stays anchor-only; file-local rename is a language-server
  operation until a CLI need appears.
- **Warning severity in @[Check].** Unused aliases would be the first warning. @[Coverage] is the
  existing advisory channel, and `--strict` already has a meaning.
