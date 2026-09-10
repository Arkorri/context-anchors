# Changelog

Notable changes to `context-anchors`. This project follows [semantic versioning](https://semver.org).

## [0.0.1] - 2026-09-09

First published release. The commands below work, but this version exists mainly to exercise the
release pipeline end to end and to claim the package names — treat the CLI surface as unstable
until 0.1.0.

### Added

- `anchr check` — resolves every `@anchor`, `@ref`, and `@[alias]` marker under the project root
  and reports what does not resolve. Supports `--strict`, `--format json`, and narrowing to
  specific paths.
- `anchr backrefs` — lists every reference to a target.
- `anchr rename` — renames an anchor id, rewriting its declaration and every reference to it.
- `anchr coverage` — reports reference-shaped strings that carry no marker. Never fails.
- `anchr annotate` — proposes markers for reference-shaped strings whose target already resolves.
- `anchr init` — writes `anchr.toml`, the marker guide for agents, and optional editor hooks.
- `anchr lsp` — a Language Server Protocol server over stdio.
- `anchr completions` — shell completion scripts.
- Source parsing for Go, JavaScript, Python, Rust, and TypeScript through tree-sitter, and for
  Markdown through pulldown-cmark.
- Prebuilt binaries for macOS (Apple Silicon and Intel), Linux (x86_64 and arm64, both glibc and
  musl), and Windows x86_64 — installable by shell script, PowerShell, or npm.
