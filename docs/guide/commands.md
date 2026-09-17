---
title: Commands
description: Every anchr command with its options, an example run, and the exit codes scripts can rely on.
tags: [guide, cli, commands]
---

# Commands

<!-- refs -->
@ref[docs/guide/markers.md as MarkersPage]
@ref[docs/guide/configuration.md as ConfigurationPage]
@ref[docs/guide/continuous-integration.md as CiPage]
@ref[docs/guide/editors.md as EditorsPage]
@noref[anchr.toml, ANCHR.md, src/parse.rs]

| Command | Does |
|---|---|
| `anchr check` | resolves every reference and reports what does not resolve |
| `anchr coverage` | lists strings that look like references but carry no marker |
| `anchr annotate` | adds markers where the target already exists |
| `anchr backrefs` | lists every reference to one target |
| `anchr rename` | renames an anchor id everywhere |
| `anchr init` | writes the config file and the marker guide |
| `anchr lsp` | runs the language server for editors |
| `anchr completions` | prints a shell completion script |

`anchr --version` prints the version. `anchr <command> --help` prints the options for that
command.

## Options every command takes

| Option | Meaning |
|---|---|
| `--root <DIR>` | start looking for the root from this directory instead of the current one |
| `--color <WHEN>` | `auto` (the default), `always`, or `never` |
| `--format <FORMAT>` | `human` (the default) or `json`; `check` also has `github` |

The root is found by walking up from the starting directory to the first `anchr.toml`, then to
the first `.git`, then falling back to the starting directory itself.

## `anchr check`

Resolves every reference in the root and reports each one that does not resolve, grouped by
cause, with a suggestion when one is close.

```sh
anchr check
```

```text
error: unknown anchor id `design/old-parsing` in root `my-project`
 --> docs/notes.md:4:11
  |
4 | Old plan: @ref[#design/old-parsing].
  |           ^^^^^^^^^^^^^^^^^^^^^^^^^
  |
  = help: did you mean `design/parsing`?

checked 3 references in 3 files (1 anchors): 2 resolved, 1 errors, 0 unverified
```

| Option | Meaning |
|---|---|
| `[PATHS]...` | report only references written in these files; findings about the root as a whole are still reported |
| `--strict` | treat every unverified reference as an error |
| `--format github` | the human report, then one annotation per location for GitHub Actions |
| `--format json` | the same report as structured data |

**Unverified** means anchr could not check a reference: the target file is in a language it has
no parser for, a configured root is not present on this machine, a file was too large to scan.
Unverified references are always listed and never counted as passing, but they only fail the
run with `--strict`. On a project where every reference can be checked, use `--strict`. The
`unverified` setting in `anchr.toml` makes it the default (@[ConfigurationPage]).

```text
unverified: no grammar for `src/tool.rb`; symbol references into it were not checked
 --> docs/notes.md:6:6
```

The JSON report carries `schema: 1`, a `summary` block with counts, the list of roots, and one
`diagnostics` entry per finding with its code, severity, message, suggestion, and locations.
Each location has a path, a line, and a column.

## `anchr coverage`

Lists reference-shaped strings that carry no marker: paths, and names of files and directories,
in prose and comments. It never fails; it is a to-do list, not a check.

```sh
anchr coverage
```

```text
`src/parse.rs` — could be @ref[src/parse.rs]
 --> docs/design.md:6:21

3 of 6 reference-shaped strings are annotated; 3 could be, 0 do not resolve
```

| Option | Meaning |
|---|---|
| `[PATHS]...` | look for candidates only in these files |

A candidate is reported as "could be" when its target exists, and "does not resolve" when it
does not, which usually means a typo or a file that has since moved. A declared name that is
never used, and a `@noref` entry that never matches anything, are listed too. See @[MarkersPage] for
how to silence a candidate that is not a reference.

## `anchr annotate`

Turns coverage candidates into markers. Only a candidate whose target resolves is changed, so the
result passes `anchr check` if it passed before.

```sh
anchr annotate
```

```text
docs/design.md:6:21: src/parse.rs -> @ref[src/parse.rs]
docs/design.md:7:26: anchr.toml -> @ref[anchr.toml]
3 proposals; pass --write to apply
```

| Option | Meaning |
|---|---|
| `[PATHS]...` | propose markers only in these files |
| `--write` | apply the proposals; without it, nothing is written |

## `anchr backrefs`

Lists every place that points at a target. Useful before deleting or moving something.

```sh
anchr backrefs '#design/parsing'
```

```text
docs/notes.md:3:5
1 reference to `#design/parsing`
```

The argument is a target in the same form you would write inside `@ref[...]`: `#some-id`,
`src/parse.rs`, `src/parse.rs#parse_file`. Quote it in the shell, because `#` starts a comment.

## `anchr rename`

Renames an anchor id: the declaration and every reference to it, in every file.

```sh
anchr rename design/parsing design/parser --dry-run
```

```text
would edit docs/design.md (1 sites)
would edit docs/notes.md (1 sites)
would rename `design/parsing` -> `design/parser`: 1 declaration, 1 reference; run `anchr check` to confirm
```

| Option | Meaning |
|---|---|
| `--dry-run` | print the planned edits without writing anything |

Run with `--dry-run` first, then without it. Files and functions are renamed with your usual
tools; `anchr check` then points at every reference that needs updating.

## `anchr init`

Writes `anchr.toml` and `ANCHR.md` at the root. The config file has every setting, commented
out with its default. The guide is a one-page description of the markers, written for coding
agents that edit your repository.

```sh
anchr init
```

```text
created: anchr.toml
created: ANCHR.md

Add to AGENTS.md or CLAUDE.md: "Before editing docs or comments, read ANCHR.md; after editing, run `anchr check`."
```

| Option | Meaning |
|---|---|
| `--root <DIR>` | the directory to set up, instead of the current one |
| `--agent <AGENT>` | `agents-md` (the default) writes the guide; `claude` also adds a hook that runs `anchr check` after every edit; `none` writes only the config |
| `--force` | overwrite a file that exists with different contents |
| `--dry-run` | print what would be written without writing it |

`init` never overwrites an existing file unless you pass `--force`, prints every path it
touched, and does nothing the second time. The `claude` hook is described in @[EditorsPage].

## `anchr lsp`

Runs a language server over standard input and output. Your editor starts it; you do not run it
by hand. See @[EditorsPage].

## `anchr completions`

Prints a completion script for `bash`, `elvish`, `fish`, `powershell`, or `zsh`:

```sh
anchr completions zsh > ~/.zfunc/_anchr
```

## Exit codes
<!-- @anchor[guide/exit-codes] -->

| Code | Meaning |
|---|---|
| 0 | clean |
| 1 | at least one broken reference (or, with `--strict`, one unverified reference) |
| 2 | anchr could not run: a bad config file, an unreadable root, a wrong argument |

`coverage` and `backrefs` always exit 0 when they run. A CI step fails on 1 and 2 alike, which
is what you want: a check that could not run is not a passing check (@[CiPage]).
