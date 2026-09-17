---
title: Configuration
description: Every setting in the config file, what it defaults to, and when to change it.
tags: [guide, config]
---

# Configuration

<!-- refs -->
@ref[docs/guide/markers.md as MarkersPage]
@ref[docs/guide/commands.md as CommandsPage]
@noref[anchr.toml, src/**, .claude/, .git/]

All settings live in one file, `anchr.toml`, at the top of your root. `anchr init` writes it
with every setting present and commented out, so the file documents itself. Every key is
optional. The defaults below are what you get with an empty file, or with no file at all.

```toml
[root]
name = "my-project"

[roots]
specs = "../specs"

[scan]
include = []
max-file-bytes = 2097152
parse-budget-ms = 5000

[ignore]
paths = []
tokens = []

[containers]
markdown = ["md", "markdown"]
plaintext = ["txt"]

[check]
unverified = "report"
```

## `[root]`

| Key | Default | Meaning |
|---|---|---|
| `name` | the directory's name | how other roots refer to this one, and how it appears in reports |

The name may contain letters, digits, `_`, and `-`. It only matters once a second root points
here.

## `[roots]`: other places references can reach

Each entry names another directory that references in this root may point into, with a
`name:` prefix on the target:

```toml
[roots]
specs = "../specs"
claude = "~/.claude"
```

With that, `@ref[specs:#auth/flow]` means "an anchor `auth/flow` exists in the `specs` root", and
`@ref[claude:skills/review.md]` means that file exists under `~/.claude`. Relative paths are
resolved from the config file, and `~` is your home directory.

A root that is not present on the machine running the check makes every reference into it
unverified rather than broken, because "not here" is a different problem from "gone". See
@[CommandsPage] for what unverified means.

## `[scan]`: which files are read

| Key | Default | Meaning |
|---|---|---|
| `include` | `[]` | patterns a file must match to be scanned; empty means every file anchr understands |
| `max-file-bytes` | `2097152` | a larger file is reported as unverified instead of being scanned |
| `parse-budget-ms` | `5000` | how long anchr will spend parsing one source file before giving up on it |

`include` narrows the scan without touching what counts as a target: a file outside `include`
is not read for markers but still exists as something a reference can point at. To remove a
file entirely, use `[ignore] paths`.

## `[ignore]`: what to skip and what not to suggest

Two keys answer two different questions.

**`paths`: which files does anchr never look at?** The syntax is that of a `.gitignore`, and
the list is applied on top of your real `.gitignore`. A listed path is not scanned, not
checked, and does not exist as a target.

```toml
[ignore]
paths = [".venv/", "vendor/", "docs/generated/**"]
```

Hidden directories such as `.claude/` are scanned like any other, because agent instructions
are exactly the kind of prose anchr is for. Only `.git/` is always skipped. Add the hidden
directories you want skipped here.

**`tokens`: which strings does `anchr coverage` never suggest?** Each entry is a pattern matched
against the whole string.

```toml
[ignore]
tokens = ["CLAUDE.md", "package.json", "src/**"]
```

`CLAUDE.md` matches only that word. `src/**` matches everything under `src/`, and `**/CLAUDE.md`
matches that name at any depth. Nothing is a prefix unless you write `**`. This is the
whole-project version of `@noref` (@[MarkersPage]), which does the same for one file.

## `[containers]`: which extensions hold prose

| Key | Default | Meaning |
|---|---|---|
| `markdown` | `["md", "markdown"]` | extensions read as Markdown |
| `plaintext` | `["txt"]` | extensions read as plain text, where a marker can appear anywhere |

Source files are recognised by extension and are not configurable: `rs`, `ts`, `tsx`, `mts`,
`cts`, `js`, `jsx`, `mjs`, `cjs`, `py`, `pyi`, and `go`. Markers in them are read from comments.

## `[check]`: how strict the check is

| Key | Default | Meaning |
|---|---|---|
| `unverified` | `"report"` | `"error"` makes every unverified reference fail the check, as `--strict` does on the command line |

Set it to `"error"` once every reference in the project can be checked, so nobody has to
remember the flag.
