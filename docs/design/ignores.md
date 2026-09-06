# Ignore lists — saying "this is not a reference"

**Status:** implemented. Companion to @ref[docs/design/aliases.md] (the construct that made the
remaining coverage candidates visible) and @ref[CODE_DESIGN.md] (the pipeline this extends).
<!-- refs -->
@ref[#cli/check as Check]
@ref[#cli/coverage as Coverage]
@ref[#cli/annotate as Annotate]
@ref[#cli/backrefs as Backrefs]
@ref[#cli/rename as Rename]
@ref[crates/anchr-core/src/marker/noref.rs#NoRefEntry as NoRefEntry]
@ref[crates/anchr-core/src/noref.rs#NoRefSet as NoRefSet]
@ref[crates/anchr-core/src/config.rs#CoverageConfig as CoverageConfig]
@noref[foo.ts, src/file.ts, line/col]

## 1. Problem

@[Coverage] reports every reference-shaped string that carries no marker. After aliases landed
and the genuine misses in this repository were annotated, about two thirds of what remained was
classified *correctly* and was still not a reference: archived research documents, files that
exist in a user's repository rather than this one (`CLAUDE.md`, `AGENTS.md`), example paths in
templates and READMEs, and the product name used as a word. No tokenizer can tell those apart
from real misses. Only the author can, and the author had no way to say so, which meant the
report never reached zero and stopped being read.

The remaining third (`line/col` prose, short identifiers matched to the wrong declaration) is a
classifier defect and is not what this design is for. The rule for authors: **if the tool guessed
wrong, fix the tool; if it guessed right and you disagree, ignore it.** An ignore list that
absorbs classifier bugs hides real mentions elsewhere.

## 2. Two axes

- **Global.** Strings that are never references anywhere in the root, declared once in
  `anchr.toml`. Also whole files that should stay checked but never be asked for more annotation.
- **Local.** Strings that are not references *in one file*, declared in that file. An example
  path in one document may be a real file in another, and the ignore should travel with the text
  it protects: move or delete the file and the ignore goes with it.

Site-level escapes were rejected: the strings in question sit inside inline code in rendered
prose, and there is no way to mark one without changing what the reader sees. File scope is the
finest granularity that leaves the document alone.

## 3. Grammar
<!-- @anchor[ignores/grammar] -->

```
marker      := anchor | ref | use | noref
noref       := "@noref[" entry ( "," ws* entry )* "]"
entry       := 1..=256 bytes, no whitespace, none of  , [ ] @ `
```

```toml
[coverage]
exclude = ["docs/research/**"]
ignore  = ["CLAUDE.md", "AGENTS.md", "anchr"]
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
- **Entries are plain strings, not targets.** No `#` grammar, no `root:` prefix, no filesystem
  check. Most of them name things that do not exist; that is the point. The forbidden characters
  are the ones that would be ambiguous inside a marker body or that no coverage token contains.
- **Case-sensitive**, because coverage tokens are.
- **Duplicates are not errors.** The second copy can never match anything and is reported as
  unused (§5). @[Check] gains no diagnostic from this feature.

## 4. Semantics
<!-- @anchor[ignores/semantics] -->

**Config.** `exclude` is a list of globs with the same syntax as `[scan] exclude`. A matching file
is still scanned, indexed, and checked; @[Coverage] generates no candidates for it and counts
none of its references. `[scan] exclude` remains the knob for "invisible to anchr". `ignore` is a
list of entries validated by the same @[NoRefEntry] parser as the marker, so a config entry with
whitespace, a comma, or a duplicate is a config error with a span. `[coverage]` is read from the
current root only; external roots never run coverage. See @[CoverageConfig].

**Matching.** A @[NoRefSet] is built once from config and once per file from that file's
`@noref` markers. A token is dropped when either set claims it, the local set first. The token is
the candidate text with surrounding backticks stripped, so authors write `foo.ts`, never
`` `foo.ts` ``. An entry `e` matches a token `t` when:

1. `t == e`;
2. `t` has the shape `path#symbol` and `path == e`, so ignoring a file ignores every symbol
   mention qualified by it;
3. `e` ends in `/` and `t`, or its path under rule 2, starts with `e`. The same trailing-slash
   convention directory references use.

Nothing else: no substrings, no globs, no case folding, and a bare `Name` does not ignore
`src/file.ts#Name`. An ignore list is a vocabulary, and vocabularies are exact. If templates with
many placeholder paths ever make globs necessary, that is a separate key, not a `*` in this one.

Matching applies to every candidate source: path tokens, identifier tokens, and alias-word
matches. `@noref[Scope]` therefore also suppresses an alias `Scope` matching English prose,
although renaming the alias is the better fix.

**Counting.** Suppressed tokens are counted as `ignored` and left out of the total: the author has
said they are not reference-shaped. Excluded files contribute nothing to either side of the
ratio.

## 5. Unused entries

An ignore list that nobody audits accumulates entries for text that no longer exists. This is how
every linter's suppression file ends up, so unused entries are reported the way unused aliases
are:

- A `@noref` entry that matched nothing in its file is a @[Coverage] candidate of kind
  `unused-ignore`, located at the entry's span inside the marker. Duplicates land here too.
- A config `ignore` entry that matched nothing in any non-excluded file is reported once as an
  `anchr.toml:` line and, in JSON, as `unused_config_ignores`. It is not a candidate: candidates
  carry a location in an indexed file, and `anchr.toml` is not one. Only a whole-root run reports
  these; a run narrowed to some files cannot see where a root-wide entry matches.
- `exclude` globs that match no file are not reported. A glob for a directory that does not
  exist yet is a normal state, as it is for `[scan] exclude`.

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
- Use config `ignore` for a string that is a non-reference everywhere; use `@noref` for examples,
  since an example path in one document may be a real file in another.
- Never ignore a wrong-target proposal. File it against the classifier.
