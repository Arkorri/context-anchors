# context-anchors

`anchr` brings compiler semantics to the prose that agentic coding runs on. `CLAUDE.md`,
`AGENTS.md`, skills, design docs, and code comments are full of references to files, functions,
and sections of other documents, and nothing checks them. They rot silently, and an agent that
follows a dead reference tends to conclude the target does not exist rather than that it moved.

Two opt-in markers make those references explicit and checkable:

```text
@anchor[some-id]     declares a stable identity at this line
@ref[target]         asserts that the target exists
```

`anchr check` resolves every reference in a root and fails when one does not resolve, grouping
the report by cause so that renaming one anchor with twelve live references is one error that
lists twelve sites. What it could not verify (a missing external root, a language it has no
grammar for) is reported as *unverified*, never silently passed.

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

## Install

Prebuilt binaries for macOS, Linux, and Windows are attached to each GitHub release.

```sh
# shell installer (macOS, Linux)
curl --proto '=https' --tlsv1.2 -LsSf \
  https://github.com/averykempton/context-anchors/releases/latest/download/context-anchors-installer.sh | sh

# npm: a native binary via a platform-specific optional dependency, no postinstall download
npx context-anchors check

# from source
cargo install --git https://github.com/averykempton/context-anchors context-anchors
```

## Use

```sh
anchr init                 # writes anchr.toml and ANCHR.md (the marker guide for agents)
anchr init --agent claude  # also wires a Claude Code PostToolUse hook that runs the check
anchr check                # exit 0 clean, 1 broken references, 2 tool failure
anchr check --format json  # stable machine-readable report
anchr check --strict       # unverified findings fail too
anchr backrefs '#auth/flow'         # every reference to a target
anchr rename auth/flow auth/session # rewrite an anchor id everywhere (--dry-run to preview)
anchr coverage             # reference-shaped strings that carry no marker; never fails
anchr annotate --write     # add @ref markers where the target resolves
anchr lsp                  # language server for any LSP-capable editor
```

The language server speaks stdio. Point your editor's generic LSP client at `anchr lsp` for
Markdown and the supported source languages to get diagnostics, go-to-definition on
`@ref[...]`, find-references and rename on anchors, and anchors as document symbols.

Targets a reference can name:

| Form | Meaning |
|---|---|
| `src/dir/` | a directory exists |
| `src/file.ts` | a file exists |
| `src/file.ts#Name` | a declaration named `Name` exists in that file (Rust, TypeScript, JavaScript, Python, Go) |
| `#some-id` | an anchor with that id exists in this root |
| `claude:#some-id` | an anchor exists in the external root named `claude` |

Markers are recognised in Markdown prose (outside code fences and inline code), in source code
comments (outside backtick spans), and in `.txt` files. Configuration lives in @ref[anchr.toml];
`anchr init` writes one with every option documented.

## Design

- @ref[DESIGN.md] — what the tool is and the guarantees it makes
- @ref[DISTRIBUTION.md] — how it ships
- @ref[CODE_DESIGN.md] — how the code is shaped, and every place it deviates from the two above
- @ref[docs/design/aliases.md] — file-scoped alias imports, so a reference is declared once per
  file and used by a short local name
- `docs/research/` — the crate survey, security checklist digest, and design review behind it

## License

MIT or Apache-2.0, at your option.
