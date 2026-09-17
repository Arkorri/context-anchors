---
title: Editors and agents
description: Live diagnostics and navigation in any editor with a language-server client, and the guide and hook that keep coding agents honest.
tags: [guide, lsp, editors, agents]
---

# Editors and agents

<!-- refs -->
@ref[docs/guide/commands.md as CommandsPage]
@noref[ANCHR.md, .claude/settings.json]

## In an editor

`anchr lsp` is a language server. Point any editor's language-server client at it and Markdown
files get:

- **Diagnostics** as you type: a broken reference is underlined, with the same message and
  suggestion `anchr check` prints.
- **Go to definition** on `@ref[...]` and `@[Name]`: jump to the file, the declaration, or the
  anchor.
- **Find references** on an anchor or a name: every place that points at it.
- **Rename** on an anchor id or a name: the declaration and every use, across files.
- **Document symbols**: an outline of everything the file declares.

The server speaks over standard input and output, so the configuration is the same in every
editor: run the command `anchr`, with the argument `lsp`, for Markdown files. Where you write
that depends on the editor. Most have a generic language-server setting or plugin that takes a
command and a list of file types.

The server and the command line share one engine, so what the editor shows is what CI will
report.

## With coding agents

Agents edit prose as readily as code, and a renamed file leaves stale mentions behind just the
same. `anchr init` writes `ANCHR.md`, a one-page guide to the markers written for an agent, and
prints the line to add to your `AGENTS.md` or `CLAUDE.md`:

```text
Before editing docs or comments, read ANCHR.md; after editing, run `anchr check`.
```

For Claude Code, `anchr init --agent claude` also adds a hook to `.claude/settings.json` that
runs `anchr check` after every file edit. When the check fails, its report is fed back to the
agent, which sees the broken reference at once and fixes it in the same turn. The hook is merged
into the settings file without disturbing anything else in it, and it is not added twice.

The options of `init` are in @[CommandsPage].
