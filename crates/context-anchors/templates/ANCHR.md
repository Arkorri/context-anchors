# Reference markers (`anchr`)

This project uses two markers so that references in prose and code comments can be checked
like code. `anchr check` fails when a reference no longer resolves and reports every place
that still uses the old name.

## Markers

```text
@anchor[some-id]        declares a stable identity at this line
@ref[target]            asserts that the target exists
@ref[target as Alias]   the same, and names the target Alias within this file
@[Alias]                a use of that name; checked through its declaration
@noref[a, b/, c.ts]     these strings are not references in this file (coverage only)
```

Markers are recognised in Markdown prose (not inside code fences or inline code), in source
code comments (not inside backtick spans), and anywhere in `.txt` files. To show a marker as
an example without it being checked, put it in a code fence or inline code, or escape it:
`\@ref[...]` or `@ref\[...\]`.

## Targets
<!-- @noref[src/, auth/token-refresh, file.rs] -->

| Form | Meaning |
|---|---|
| `src/dir` or `src/dir/` | the path exists (trailing `/` requires a directory) |
| `src/file.ts` | the file exists |
| `src/file.ts#Name` | a declaration named `Name` exists in that file |
| `#some-id` | an `@anchor[some-id]` exists in this root |
| `root:#some-id` | an `@anchor[some-id]` exists in the external root `root` |

Rules:

- Paths are relative to the root (the directory holding `anchr.toml`, or the git root),
  never to the file the reference is in. `..` is not allowed.
- Path lookups are exact: `src/Foo.ts` does not match `src/foo.ts`.
- Anchor ids use letters, digits, `_`, `.`, `-`, and `/` for namespacing: `auth/token-refresh`.
  An id must be unique within its root.
- Symbol names are unqualified: write `file.rs#method`, not `file.rs#Type::method`.
  The check asks "does a declaration with this name exist anywhere in this file?".
- A `root:` prefix works on every target form.

## Aliases

When a file mentions one target many times, declare it once and use the name everywhere:

```markdown
<!-- refs -->
@ref[src/auth.ts#validateToken as ValidateToken]
@ref[#auth/flow as AuthFlow]

@[ValidateToken] runs first; @[AuthFlow] describes what happens next.
```

- Aliases are file-scoped: declare in each file that uses them. Put the declarations in an
  index block at the top of the file, under `<!-- refs -->`, like imports.
- An alias is an identifier: letters, digits, `_`, starting with a letter or `_`.
- A use with no declaration in the file is an error; so is declaring one alias twice in a file.
  An alias that is declared and never used shows up in `anchr coverage`.
- If a target breaks, the error is reported at the declaration, once. Fix that line; the uses
  keep their name.
- Name the thing the way this document talks about it. The alias does not have to match the
  code's spelling, so a rename in code is one edit per file.

## Ignores

`anchr coverage` lists reference-shaped strings that carry no marker. When one is correctly
shaped and still not a reference (an example path, a file that lives in another repository),
say so once and the report stops asking:

```markdown
<!-- refs -->
@noref[src/legacy/, example.ts]
```

- `@noref` is file-scoped, like an alias. For strings that are never references anywhere, use
  `ignore` under `[coverage]` in `anchr.toml`; `exclude` there keeps whole files checked but never
  proposes annotations in them.
- Entries are plain strings separated by commas: exact match, or the path of a `path#Name`
  token, or a prefix when the entry ends in `/`. No globs.
- An entry that matches nothing shows up in `anchr coverage`, like an unused alias. `anchr check`
  never reports on ignores.

## When editing

- Renaming or moving a file, function, or type, or changing an anchor id, breaks every
  reference to it. Run `anchr check` afterwards and fix each reported site, or use the
  suggested replacement.
- Rewording a heading does not break anything: identity lives in the `@anchor[...]`, not
  the heading text.
- To make something referenceable, add an `@anchor[...]` where it lives, then refer to it
  with `@ref[#...]`.

## Commands

```text
anchr check                    human-readable report; exit 1 on errors
anchr check --format json      machine-readable report (schema 1)
anchr check --strict           unverified findings (missing root, no grammar) also fail
anchr check path/to/file.md    report only references in the given files
anchr backrefs '#some-id'      every reference to a target
anchr rename old-id new-id     rewrite an anchor id and every reference to it (--dry-run first)
anchr coverage                 reference-shaped strings with no marker; never fails
anchr annotate [--write]       propose (or apply) @ref markers where the target resolves
anchr lsp                      language server: diagnostics, go-to-definition, references, rename
                               (anchors and aliases)
```

Exit codes: 0 clean, 1 broken references, 2 the tool could not run (bad config).
