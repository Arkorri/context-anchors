# Ignores — which paths anchr looks at, which strings it proposes

**Status:** implemented. Companion to @ref[./aliases.md] (the construct that made the
remaining coverage candidates visible) and @ref[CODE_DESIGN.md] (the pipeline this extends).
<!-- refs -->
@ref[#cli/check as Check]
@ref[#cli/coverage as Coverage]
@ref[#cli/annotate as Annotate]
@ref[#cli/backrefs as Backrefs]
@ref[#cli/rename as Rename]
@ref[crates/anchr-core/src/marker/noref.rs#NoRefEntry as NoRefEntry]
@ref[crates/anchr-core/src/noref.rs#NoRefSet as NoRefSet]
@ref[crates/anchr-core/src/config.rs#IgnoreConfig as IgnoreConfig]
@noref[foo.ts, src/file.ts, src/x.ts, docs/CLAUDE.md, crates/x/src/lib.rs, research/, drafts/, build/, .claude/, .claude/skills/]

## 1. Problem

@[Coverage] reports every reference-shaped string that carries no marker. After aliases landed
and the genuine misses in this repository were annotated, about two thirds of what remained was
classified *correctly* and was still not a reference: files that exist in a user's repository
rather than this one (`CLAUDE.md`, `AGENTS.md`), example paths in templates and READMEs. No
tokenizer can tell those apart from real misses. Only the author can, and the author had no way
to say so, which meant the report never reached zero and stopped being read.

The remaining third was classifier defect, not ignore material: `line/col` prose read as a path
and short identifiers matched to the wrong declaration. Both are fixed since, by the path-shape
rule and by dropping bare identifiers from coverage (@ref[CODE_DESIGN.md] §12a items 15 and 16).
The rule for authors: **if the tool guessed wrong, fix the tool; if it guessed right and you
disagree, ignore it.** An ignore list that absorbs classifier bugs hides real mentions elsewhere.

A second problem arrived with the first monorepo run (§12a item 20 of @ref[CODE_DESIGN.md]):
the same two questions were answered by five knobs, named after the tool phase that read them
rather than what the author meant, and the token matcher carried a rule nobody could see in the
config.

## 2. Two questions

Everything an author can ignore answers one of two questions, and the config has one key per
question:

- **Should this path be looked at?** `[ignore] paths`. A listed path is not scanned, not
  checked, and does not exist as a reference target. There is no softer form: a file is looked
  at or it is not.
- **Should this string be proposed?** `[ignore] tokens` root-wide, `@noref` in one file. A
  matched string is never proposed as a reference; nothing else about it changes.

`@noref` is **file-scoped**, like an alias. An example path in one document may be a real file
in another, and the ignore should travel with the text it protects: move or delete the file and
the ignore goes with it. Site-level escapes were rejected: the strings in question sit inside
inline code in rendered prose, and there is no way to mark one without changing what the reader
sees. File scope is the finest granularity that leaves the document alone.

## 3. Grammar
<!-- @anchor[ignores/grammar] -->

```
marker      := anchor | ref | use | noref
noref       := "@noref[" entry ( "," ws* entry )* "]"
entry       := glob, 1..=256 bytes, no whitespace, neither @ nor `
```

```toml
[ignore]
paths  = ["target/**", ".venv/**"]                 # gitignore syntax
tokens = ["CLAUDE.md", "AGENTS.md", "src/**"]      # globs matched against the whole token
```

- **`@noref`, not `@anchor-ignore`.** The marker asserts "these strings are not references" and
  says nothing about anchors. It joins the kind alternation of the one lexer regex, so `@norefs[`
  and `@NoRef[` are not markers, `\@noref[` and `foo@noref[` are escaped or glued exactly like
  every other marker, and `@noref ` followed by a space is never lexed because `[` must follow
  the kind immediately.
- **Comma is the separator, and whitespace is allowed only after a comma.** Entries contain no
  whitespace, so `@noref[a, b]` and `@noref[a,b]` are the same list, while `@noref[a b]` is
  malformed with a message that names commas. Leading or trailing whitespace in the body is
  malformed, matching `@ref[ x ]`. A trailing comma is malformed too: the empty entry is an
  error, not silently dropped, because the likely cause is a deleted entry that left its comma.
- **Entries are globs, not targets.** No `#` grammar, no `root:` prefix, no filesystem check.
  Most of them name things that do not exist; that is the point. The full `globset` syntax is
  accepted in both places (`*`, `**`, `?`, `[ab]`, `{a,b}`), with `*` confined to one path
  segment. One lexical fact follows from the marker shape rather than from any rule: a marker
  body cannot contain `[`, `]`, or a bare comma, so character classes and alternation are
  spellable only in `anchr.toml`. The matcher is the same; write two entries instead.
- **Case-sensitive**, because coverage tokens are.
- **Duplicates are not errors.** The second copy can never match anything and is reported as
  unused (§5). @[Check] gains no diagnostic from this feature.

## 4. Semantics
<!-- @anchor[ignores/semantics] -->

**Paths.** `[ignore] paths` is a list of gitignore lines rooted at the root directory, compiled
by the same crate that reads `.gitignore`, so anchoring (`/build/`), directory-only patterns
(`drafts/`), and negation (`!vendor/keep.md`) mean exactly what they mean in a `.gitignore` in
that directory. An unanchored name matches at any depth, as in git: `research/` removes every
directory of that name, `/docs/research/` only the one at the root. The list is layered on top
of `.gitignore`: the walker applies git's rules first and this list second, so a `!` line here
can never resurrect a gitignored file. `.gitignore` itself applies as git applies it, only
inside a git repository and with parent files stopping at the nearest `.git`; a root that is not
a repository has `paths` as its only knob.

Invisible is total. A listed path is not scanned, not indexed, and does not exist as a reference
target, exactly like a gitignored one, and a `@ref` into it fails with a note naming the line
that removed it. Ignore a tree only when nothing should reference into it.

| knob | exists as a target | indexed and checked | coverage proposes |
|---|---|---|---|
| gitignored or listed in `[ignore] paths` | no | no | no |
| matched by `[ignore] tokens` or `@noref` | n/a: a string, not a file | n/a | no |

**Tokens.** `[ignore] tokens` is a list of entries validated by the same @[NoRefEntry] parser as
the marker, so a config entry with whitespace, an invalid glob, or a duplicate is a config error
with a span. It is read from the current root only; external roots never run coverage. See
@[IgnoreConfig].

**Matching.** A @[NoRefSet] is built once from config and once per file from that file's
`@noref` markers. A token is dropped when either set claims it, the local set first. The token is
the candidate text with surrounding backticks stripped, so authors write `foo.ts`, never
`` `foo.ts` ``. An entry `e` matches a token `t` when its glob matches the whole of `t`, or `t`
has the shape `path#symbol` and the glob matches the whole of `path`, so ignoring a file ignores
every symbol mention qualified by it. Nothing is implied:

| entry | claims | leaves alone |
|---|---|---|
| `src/` | `src/` | `src/x.ts`, `src/x.ts#run` |
| `src/**` | `src/`, `src/x.ts`, `src/a/b.ts`, `src/*` | `crates/x/src/lib.rs` |
| `src/*` | `src/`, `src/x.ts` | `src/a/b.ts` |
| `CLAUDE.md` | `CLAUDE.md`, `CLAUDE.md#Setup` | `docs/CLAUDE.md` |
| `**/CLAUDE.md` | `CLAUDE.md`, `docs/CLAUDE.md` | `CLAUDE.md.bak` |

A trailing `**` matches zero segments, which is why `src/**` also claims the bare `src/`, and a
single `*` may match an empty segment, which is why `src/*` does too. No substrings, no case
folding, and a bare `Name` does not ignore `src/file.ts#Name`.

Matching applies to both candidate sources: path tokens and alias-word matches. `@noref[Scope]`
therefore also suppresses an alias `Scope` matching English prose, although renaming the alias is
the better fix.

**Counting.** Suppressed tokens are counted as `ignored` and left out of the total: the author has
said they are not reference-shaped. Ignored paths contribute nothing to either side of the ratio,
because they were never scanned.

## 5. Unused entries

An ignore list that nobody audits accumulates entries for text that no longer exists. This is how
every linter's suppression file ends up, so unused entries are reported the way unused aliases
are:

- A `@noref` entry that matched nothing in its file is a @[Coverage] candidate group of kind
  `unused-ignore`, its site the entry's span inside the marker. Duplicates land here too.
- A `[ignore] tokens` entry that matched nothing in any scanned file is reported once, with
  `anchr.toml` as its only site, and in JSON as `unused_config_ignores`. It is not a candidate:
  candidates carry a location in an indexed file, and `anchr.toml` is not one. Only a whole-root
  run reports these; a run narrowed to some files cannot see where a root-wide entry matches.
- `[ignore] paths` lines that match no file are not reported. A line for a directory that does
  not exist yet is a normal state, as it is in `.gitignore`.

"Matched" means "would have been a candidate had it not been ignored", decided during candidate
generation rather than by a second text scan. An entry whose only occurrence is inside a code
fence is unused, correctly: it protects nothing.

## 6. Interaction with the rest of the pipeline

- @[Check] lexes `@noref` like any marker, so malformed forms are errors; it otherwise never sees
  one. No new diagnostic kind, `--strict` unchanged.
- @[Backrefs], @[Rename], and the language server treat `@noref` markers as nothing: no target,
  no id, not a document symbol.
- @[Annotate] acts on proposals only; its output shrinks, its behaviour does not change.
- The marker's own span is already excluded from candidate scanning, so its entries are never
  proposed against themselves.
- Invariants (@ref[DESIGN.md] §8): deterministic (a pure function of config and file bytes), opt-in
  (nothing is ignored unless written), unverifiable never renders valid (ignores remove
  candidates, they never mark anything resolved), grouped by cause (one advisory per entry).

## 7. Conventions
<!-- @anchor[ignores/conventions] -->

- Put `@noref` in the index block at the top of the file, after the alias declarations, under
  `<!-- refs -->`. In source files, a `//` comment near the top or next to the example it
  protects.
- Use `[ignore] tokens` for a string that is a non-reference everywhere; use `@noref` for
  examples, since an example path in one document may be a real file in another.
- Spell the subtree when you mean it: `src/**`, never `src/` hoping for a prefix. A directory
  you do want referenced beneath can then be ignored as a bare word without hiding its files.
- In `paths`, anchor what you mean: `/build/` for the one at the root, `build/` only if every
  directory of that name should go.
- Never ignore a wrong-target proposal. File it against the classifier.

## 8. Superseded decisions

The first version of this design had `[coverage] ignore` (exact match, plus the path half of
`path#Symbol`, plus a prefix match for entries ending in `/`), `[coverage] exclude` (files
checked but never proposed), `[scan] exclude`, and `.anchrignore`, and said "vocabularies are
exact; if globs are ever needed, that is a separate key". Three things changed it:

- The trailing-slash rule had already broken exactness, invisibly: `src/` in this repository's
  own config suppressed every `src/...` mention, and `.claude/` would have suppressed every
  skill under `.claude/skills/`. An explicit `**` is what the rule was pretending not to be.
- Five knobs for two questions. `[scan] exclude` was even compiled twice, as globset for the
  "why is this missing" note and as gitignore for the walk, with two different syntaxes.
- "Checked but never proposed" earned no place. Its one use here, archived research, is served
  by annotating the handful of real references and `@noref`-ing the rest.
