---
title: Getting started
description: What anchr does, how to install it, and the first four commands to run in your own project. Read this page first.
tags: [guide, install, quickstart]
---

# Getting started

<!-- refs -->
@ref[docs/guide/markers.md as MarkersPage]
@ref[docs/guide/commands.md as CommandsPage]
@ref[docs/guide/configuration.md as ConfigurationPage]
@ref[docs/guide/continuous-integration.md as CiPage]
@ref[docs/guide/editors.md as EditorsPage]
@noref[anchr.toml, ANCHR.md, src/parse.rs]

## What anchr does

Documentation is full of sentences that point at code: "the parser lives in `src/parse.rs`",
"see the design doc for the plan". When the code changes, nothing tells you those sentences are
now wrong. You find out when someone follows one and finds nothing there.

`anchr` makes those sentences checkable. You mark a reference, and `anchr check` fails the day it
stops pointing at something real:

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

A rename with twelve references to it is one error that lists twelve places to fix.

## Install

Pick whichever fits your project. Every option installs the same binary.

**In a project that uses npm.** The checker is pinned in your lockfile, so everyone on the team
and every CI run gets the same version:

```sh
npm install --save-dev context-anchors
```

Installed this way, `anchr` is not on your `PATH`. Run it as `npx context-anchors`, or add a
script to `package.json`.

**With Homebrew**, on macOS or Linux:

```sh
brew install Arkorri/tap/context-anchors
```

**With the installer script.** It downloads the release for your platform and puts `anchr` on
your `PATH`:

```sh
# macOS, Linux
curl --proto '=https' --tlsv1.2 -LsSf \
  https://github.com/Arkorri/context-anchors/releases/latest/download/context-anchors-installer.sh | sh

# Windows
powershell -ExecutionPolicy Bypass -c "irm https://github.com/Arkorri/context-anchors/releases/latest/download/context-anchors-installer.ps1 | iex"
```

**From source**, with a Rust toolchain:

```sh
cargo install --git https://github.com/Arkorri/context-anchors context-anchors
```

Check the install with `anchr --version`. This guide describes the latest release.

## Your first check

Run these from the top of your project.

**1. Create a config file.**

```sh
anchr init
```

This writes two files. `anchr.toml` holds the settings, all commented out because the defaults
are usually right. `ANCHR.md` is a short guide to the markers, written for coding agents that
work in your repository. The directory holding `anchr.toml` becomes your **root**: the tree
anchr reads, and the boundary references resolve inside.

**2. See what could be checked.**

```sh
anchr coverage
```

anchr reads your Markdown, plain-text files, and code comments, and lists every string that
looks like a reference but is not yet marked:

```text
`src/parse.rs` — could be @ref[src/parse.rs]
 --> docs/design.md:6:21
`anchr.toml` — could be @ref[anchr.toml]
 --> docs/design.md:7:26

3 of 6 reference-shaped strings are annotated; 3 could be, 0 do not resolve
```

**3. Mark them.**

```sh
anchr annotate --write
```

Every string from step 2 whose target really exists gets a marker. `src/parse.rs` in your
design doc becomes `@ref[src/parse.rs]`, which reads the same and is now checked.

**4. Check.**

```sh
anchr check
```

```text
checked 3 references in 3 files (0 anchors): 3 resolved, 0 errors, 0 unverified
```

Now rename `src/parse.rs` and run `anchr check` again. It fails, and the error names the file and
line that still use the old name. That is the whole loop: mark once, and every later change is
caught.

## Five words you will see everywhere

- **Root.** The directory anchr reads. It is the one holding `anchr.toml`, or failing that the
  top of the git repository, or failing that the current directory.
- **Reference.** A marked mention of something: `@ref[src/parse.rs]`.
- **Target.** The thing a reference points at: a file, a directory, a declaration in a source
  file, or an anchor.
- **Anchor.** A stable name you give to a line, with `@anchor[some-id]`, so other documents can
  point at that spot even after the heading above it is reworded.
- **Unverified.** A reference anchr could not check, for example one into a language it cannot
  parse. It is reported but does not fail the check unless you ask it to.

## Where next

- @[MarkersPage]: every marker, and everything a reference can point at.
- @[CommandsPage]: every command and its options.
- @[ConfigurationPage]: every setting in `anchr.toml`.
- @[CiPage]: fail the build on a broken reference.
- @[EditorsPage]: live diagnostics in your editor, and hooks for coding agents.
