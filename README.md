# context-anchors

[![CI](https://github.com/Arkorri/context-anchors/actions/workflows/ci.yml/badge.svg)](https://github.com/Arkorri/context-anchors/actions/workflows/ci.yml)
[![npm](https://img.shields.io/npm/v/context-anchors)](https://www.npmjs.com/package/context-anchors)

**Your docs point at your code. `anchr` tells you when they stop matching.**

Design docs, READMEs, and code comments are full of references to files, functions, and sections
of other documents. Rename a function, and every mention of it in prose quietly goes stale.
Nothing fails and nothing warns you — you find out when someone follows a reference to something
that is no longer there.

`anchr` makes those references checkable. You mark one, and it fails the day it stops resolving,
the same way a compiler catches a call to a function you deleted.

```text
error: unknown anchor id `auth/flow` in root `repo`
 --> docs/a.md:3:5
  |
3 | See @ref[#auth/flow] and again @ref[#auth/flow].
  |     ^^^^^^^^^^^^^^^^
 ::: docs/b.md:3:1
 ::: src/x.rs:1:4
help: did you mean `auth/token-refresh`?
```

One rename with twelve live references is a single error listing twelve places to fix, not twelve
separate errors to wade through.

## Install

```sh
npm install --save-dev context-anchors
```

This pins the checker in your lockfile, so every developer and every CI run uses the same version.
You get a prebuilt native binary, chosen for your platform at install time — there is no
postinstall download, so it works offline and under `--ignore-scripts`.

Use `npm ci` to install. Rebuilding a lockfile on top of an existing `node_modules` can record
only your own platform's binary (npm/cli#4828), leaving teammates on other machines without one.
If that happens, `anchr` tells you which package is missing.

Don't have Node? These put a standalone binary on your `PATH`, with no Node involved at all:

```sh
# macOS, Linux
curl --proto '=https' --tlsv1.2 -LsSf \
  https://github.com/Arkorri/context-anchors/releases/latest/download/context-anchors-installer.sh | sh

# Windows
powershell -ExecutionPolicy Bypass -c "irm https://github.com/Arkorri/context-anchors/releases/latest/download/context-anchors-installer.ps1 | iex"
```

Binaries for every supported platform are attached to each GitHub release, or you can build from
source with `cargo install --git https://github.com/Arkorri/context-anchors context-anchors`.

## Quick start

```sh
anchr init     # writes a config file and a short guide to the markers
anchr check    # exit 0 clean, 1 broken references, 2 something went wrong
```

`anchr init` creates @ref[anchr.toml] at the top of your project. That directory becomes your
**root**: the tree anchr reads, and the boundary references resolve inside.

Now add a reference to a document. This one says the file must exist:

```markdown
The parser lives in @ref[src/parse.rs].
```

Delete or move that file and `anchr check` fails, naming every document that mentioned it.

Run it in CI the same way you run a linter, and a broken reference fails the build.

## Writing markers

Four markers, all opt-in — anchr only checks what you mark.

```text
@anchor[some-id]     gives this line a stable name
@ref[target]         asserts the target exists
@ref[target as X]    the same, and names the target X for this file
@[X]                 a use of that name; every mention is checked
```

A reference can point at any of these:
<!-- @noref[src/file.ts, ./sibling.md] -->

| Form | Meaning |
|---|---|
| `src/dir/` | a directory exists |
| `src/file.ts` | a file exists |
| `src/file.ts#Name` | a declaration named `Name` exists in that file (Rust, TypeScript, JavaScript, Python, Go) |
| `./sibling.md`, `../lib/x.ts#Name` | the same forms, resolved from the folder of the file the reference is in |
| `#some-id` | an `@anchor` with that id exists somewhere in your root |
| `specs:#some-id` | an anchor exists in a separate root you configured, here named `specs` |

`@anchor` is what you use when the thing you want to point at isn't a file or a function — a
section of a design doc, a step in a checklist, a paragraph someone else's notes depend on.

Markers work in Markdown prose, in code comments, and in `.txt` files. Inside code fences and
backticks they are ignored, so you can write about them without triggering them.

### Naming a target once

A document that mentions the same thing repeatedly can declare it once, at the top, and then use a
short name:

```markdown
<!-- refs -->
@ref[src/file.ts#Name as Name]

@[Name] is checked at every mention, and renaming it in code is one edit per file.
```

### Silencing false positives

`anchr coverage` points out strings that look like references but carry no marker. When one is
genuinely just an example, list it in `@noref` and it stops being suggested:

```markdown
@noref[src/legacy/, example.ts]
```

Entries are globs matched against the whole string, so `src/**` covers a subtree while `src/`
covers only that exact word. To silence something everywhere instead of in one file, put it under
`[ignore] tokens` in @ref[anchr.toml].

## Commands

```sh
anchr check                          # resolve every reference; fails if one is broken
anchr check --format json            # the same report, machine-readable
anchr check --strict                 # also fail on what couldn't be verified
anchr backrefs '#auth/flow'          # list everything pointing at a target
anchr rename auth/flow auth/session  # rename an anchor everywhere (--dry-run to preview)
anchr coverage                       # suggest references you haven't marked; never fails
anchr annotate --write               # add @ref markers where the target already resolves
anchr lsp                            # language server, for editors
```

Anything anchr could not verify — a language it has no parser for, a root it cannot reach — is
reported as **unverified** rather than quietly passing. Those don't fail the run unless you pass
`--strict`.

Installed as a devDependency, `anchr` is not on your `PATH`: prefix these with `npx`, or put them
in a `package.json` script. The standalone installers and `npm install --global` give you the bare
command.

## Editor support

`anchr lsp` speaks stdio, so point any editor's generic LSP client at it. You get live diagnostics,
go-to-definition on `@ref[...]` and `@[...]`, find-references and rename on anchors and names, and
a document outline of everything a file declares.

## Configuration

Everything lives in @ref[anchr.toml], which `anchr init` writes for you with every option
documented — extra roots, which paths to skip, and which strings to never suggest.

## License

MIT or Apache-2.0, at your option: @ref[LICENSE-MIT] and @ref[LICENSE-APACHE].
