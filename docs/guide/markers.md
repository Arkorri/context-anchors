---
title: Markers
description: The five markers you write in prose and comments, everything a reference can point at, and the rules for paths, anchors, names, and ignores.
tags: [guide, markers, syntax]
---

# Markers

<!-- refs -->
@ref[docs/guide/commands.md as CommandsPage]
@ref[docs/guide/configuration.md as ConfigurationPage]
@noref[anchr.toml, src/file.ts, src/dir/, x.md, src/Foo.ts, src/foo.ts, file.rs]

A marker is a short tag you write inside ordinary text. anchr only checks what you mark;
everything else is left alone. There are five:

| Marker | Meaning |
|---|---|
| `@ref[target]` | the target must exist |
| `@anchor[some-id]` | gives this line a stable name that other files can point at |
| `@ref[target as Name]` | the target must exist, and this file calls it `Name` from here on |
| `@[Name]` | a use of that name; checked through the declaration above |
| `@noref[a, b/]` | these strings are not references in this file, so stop suggesting them |

Markers work in Markdown prose, in code comments, and in `.txt` files. Inside code fences and
backticks they are ignored, so you can write about them, as this page does, without triggering
them.

## `@ref`: point at something

```markdown
The parser lives in @ref[src/parse.rs], and its entry point is @ref[src/parse.rs#parse_file].
```

Delete or rename the file, or the function, and `anchr check` reports every line that still
points at it. What you can put inside the brackets is listed under [Targets](#targets) below.

## `@anchor`: name a spot

Files and functions already have names. A section of a design doc, a step in a checklist, or a
paragraph someone depends on does not. Give it one:

```markdown
## Parsing
<!-- @anchor[design/parsing] -->
```

Now any file in the root can write `@ref[#design/parsing]`. Rewording the heading breaks nothing,
because the identity lives in the anchor, not in the heading text. Inside an HTML comment, as
above, the anchor is invisible in rendered Markdown.

Anchor ids use letters, digits, `_`, `.`, `-`, and `/`. The `/` is for grouping, as in
`auth/token-refresh`. An id must be unique within the root, and `anchr check` reports a
duplicate as an error.

## `@ref[... as Name]` and `@[Name]`: name a target once

A document that mentions the same file five times would otherwise carry five `@ref` markers.
Instead, declare the target once at the top and use a short name after that:

```markdown
<!-- refs -->
@ref[src/parse.rs#parse_file as ParseFile]

@[ParseFile] reads the whole file first. Then @[ParseFile] walks the tree.
```

Every `@[ParseFile]` is checked. If the function is renamed, the error is reported once, at the
declaration, and fixing that one line fixes every mention.

The rules:

- A name is declared and used within one file. Another file that wants the same name declares it
  again.
- A name is letters, digits, and `_`, and it is case-sensitive.
- Using a name the file never declared is an error. Declaring the same name twice is an error.
- A name that is declared and never used shows up in `anchr coverage`.
- Put the declarations under a `<!-- refs -->` comment at the top of the file, like imports.

## `@noref`: stop a suggestion

`anchr coverage` lists strings that look like references but carry no marker. Some of them are
genuinely not references: an example path in a tutorial, a file that lives in another
repository. Say so once, and coverage stops asking:

```markdown
<!-- @noref[src/legacy/, example.ts] -->
```

Entries are patterns matched against the whole string. `src/` covers only that exact word;
`src/**` covers everything under it. `@noref` applies to the file it is written in. To silence a
string everywhere, put it under `[ignore] tokens` in `anchr.toml` (@[ConfigurationPage]).

## Targets
<!-- @anchor[guide/targets] -->

The text inside `@ref[...]` is a target. These are the forms:

| Target | Exists when |
|---|---|
| `src/dir/` | the directory exists (the trailing `/` requires a directory) |
| `src/file.ts` | the file exists |
| `src/file.ts#Name` | a declaration named `Name` exists in that file |
| `#some-id` | an `@anchor[some-id]` exists somewhere in the root |
| `specs:#some-id` | an anchor exists in another root you configured, here one named `specs` |

**Paths** are relative to the root unless they start with `./` or `../`, in which case they are
relative to the file the reference is written in. A same-directory file needs the `./`: `x.md`
always means the `x.md` at the top of the root. Lookups are exact, so `src/Foo.ts` does not match
`src/foo.ts`, and a file that git ignores does not exist as a target even when it is on disk.
That keeps a local run in agreement with a clean checkout.

**Declarations** are found by parsing the file. anchr understands Rust, TypeScript, JavaScript,
Python, and Go. A name is unqualified: write `file.rs#method`, not `file.rs#Type::method`. The
question anchr asks is "does something with this name exist anywhere in this file?" A reference
into a language anchr cannot parse is reported as unverified (see @[CommandsPage]).

**Anchors** are looked up by id across the whole root. A `root:` prefix, as in `specs:#some-id`,
looks in a second root declared in `anchr.toml`. The prefix works on every target form.

## Writing about markers

To show a marker as an example without it being checked, put it in a code fence or in backticks,
or escape it with a backslash: `\@ref[...]`. Plain `.txt` files have no code spans, so a file
that documents the syntax should be Markdown.
