# Reference markers (`anchr`)

This project uses two markers so that references in prose and code comments can be checked
like code. `anchr check` fails when a reference no longer resolves and reports every place
that still uses the old name.

## Markers

```text
@anchor[some-id]        declares a stable identity at this line
@ref[target]            asserts that the target exists
```

Markers are recognised in Markdown prose (not inside code fences or inline code), in source
code comments (not inside backtick spans), and anywhere in `.txt` files. To show a marker as
an example without it being checked, put it in a code fence or inline code, or escape it:
`\@ref[...]` or `@ref\[...\]`.

## Targets

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
```

Exit codes: 0 clean, 1 broken references, 2 the tool could not run (bad config).
