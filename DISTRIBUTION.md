# Distribution

**Status:** design draft
**Companion to:** @ref[DESIGN.md] — that document covers what the tool is; this one covers how it
ships.

---

## 1. Name — settled
<!-- @anchor[dist/name] -->

| Role | Name |
|---|---|
| Project, repo, npm package, crate | `context-anchors` |
| Installed command | `anchr` |
| Markers | `@anchor` / `@ref` |

Both `context-anchors` and `anchr` were free on npm and crates.io as of 2026-09-04.

The original working name `docref` was unusable: taken on npm (`docref@0.0.6`, abandoned 2022,
"Code documentation by reference") and *actively maintained* on crates.io. `doclink` was
similarly blocked on npm. Roughly 45 candidates were checked; nearly every short dictionary word
— `cleat`, `rivet`, `xref`, `tether`, `cairn`, `stele`, `datum`, `rebar`, `waymark`, `anchorage`
— is squatted on npm, which is why the viable space is compounds.

`anchorlint` was free but rejected on principle: "lint" contradicts the check-versus-lint
distinction in @ref[#design/deferred], and the name should not undercut the design.

**Remaining actions:**

- Claim `context-anchors` on npm and crates.io. crates.io has no reservation mechanism — holding
  a name there requires publishing, so push a real `0.0.1` even if it is never marketed.
- Create the npm org for the `@context-anchors` scope, which the platform packages in §4 require.
  Scope availability is not checkable from outside npm; it is confirmed at creation.
- Consider defensively reserving `anchr` on both registries so the command name cannot be
  squatted by an unrelated package.

### Package name and command name are independent

`context-anchors` is descriptive and searchable but too long to type; `anchr` is the opposite.
Both are available at once because the package name and the installed binary name are separate:
`package.json` has a `bin` map and Cargo has `[[bin]] name = "anchr"`. `npx context-anchors check`
resolves to the single bin, while installed users type `anchr`.

The obvious short form `anchor` was rejected — it is already the Solana framework's CLI, and any
developer with both installed would get a PATH collision.

---

## 2. Three artifacts, not one package

Conflating these is the usual mistake. They have different build pipelines, different channels,
and different schedules.

| Artifact | Channel | Ships |
|---|---|---|
| Binary (CLI + LSP) | GitHub Releases, npm | v1 |
| Agent integration (instructions, hooks, config) | `init` command + a Claude Code marketplace listing | v1 |
| Editor extension | VS Code Marketplace, OpenVSX | deferred |

Only the binary is a packaging problem in the conventional sense.

---

## 3. Release tooling: cargo-dist
<!-- @anchor[dist/cargo-dist] -->

[cargo-dist](https://github.com/axodotdev/cargo-dist) is the spine. Verified healthy — v0.32.0
released 2026-05-21, actively maintained.

From one config block in `Cargo.toml` plus a git tag, it generates:

- Cross-compiled binaries for every target
- A GitHub Actions release workflow
- Shell and PowerShell installer scripts
- An npm package
- A Homebrew formula pushed to a tap
- Checksums and `cargo-binstall` metadata

This is weeks of work not done. Configure it before the first release rather than retrofitting.

---

## 4. Channels
<!-- @anchor[dist/channels] -->

### Shipping in v1

**1. GitHub Releases + curl installer**

The baseline. Covers every technical user on every platform, works in any CI, no registry
account required.

**2. npm, via platform-specific `optionalDependencies`**

Structure: a tiny `anchr` package declaring optional dependencies on
`@context-anchors/darwin-arm64`, `@context-anchors/linux-x64-gnu`, and so on. npm resolves only the matching
platform package via `os`/`cpu` fields, and a thin shim execs the native binary.

**Not a postinstall download script.** Postinstall breaks under `--ignore-scripts`, in
air-gapped CI, and behind corporate proxies, and it defeats lockfile integrity. The
optionalDependencies pattern is what esbuild, swc, biome, and ruff use, and cargo-dist generates
it directly.

Two things this buys that a curl installer cannot:

- `npx anchr check` requires nothing pre-installed, so CI in a JS repository has no install
  step at all.
- `package.json` plus a lockfile **pins the version**, so every developer and CI run uses an
  identical checker. That matters for a tool that gates commits.

Node is required only to *resolve* the package, never to run the tool. The binary is native.

**3. The vendor-neutral integration layer**

`anchr init`, a git pre-commit hook, and a CI action. Detailed in §5.

**4. Claude Code marketplace listing**

A repository with `.claude-plugin/marketplace.json`, kept deliberately thin — it wraps `init`
rather than duplicating it. Worth maintaining purely for discovery, since
`/plugin marketplace add` is how those users find tools.

### Deferred, and cheap to add

**Homebrew** — cargo-dist already generates the formula; enabling a tap later is a config flip,
not a project. The curl installer covers the same audience in the meantime.

**crates.io** — not a discovery channel for CLIs; nobody browses it looking for dev tools. It
earns its place only if the resolver core is later published as an embeddable *library*. Publish
`0.0.1` for the name, revisit properly later.

**MCP server** — see §5.

**VS Code / OpenVSX** — with one nuance: deferring the *extension* is not deferring LSP reach.
Once `anchr lsp` exists, Neovim, Helix, and Zed users wire it up in a few lines of
configuration for free. Only VS Code requires an extension to speak to a generic LSP server. Ship
the subcommand in v1.1; let the extension wait for demand.

---

## 5. Agent integration portability

The requirement is that this not be Claude-specific. Decomposing what an "agent plugin" actually
contains shows the three pieces have very different portability:

| Piece | Cross-vendor? |
|---|---|
| Instructions teaching the grammar | Content portable; file location differs — `CLAUDE.md`, `AGENTS.md`, `.cursor/rules`, `GEMINI.md` |
| Automatic run-after-edit | **Not portable.** Hook systems are vendor-specific and some agents have none |
| Tool exposure | MCP is genuinely neutral — Claude Code, Cursor, Codex, and Gemini CLI all speak it |

### The neutral layer is git, not a plugin format

A **git pre-commit hook catches every broken reference regardless of what did the editing** —
Claude, Codex, Cursor, vim, a human, `sed`. Zero per-vendor work, and it covers vendors that do
not exist yet. CI is the same story one step later in the loop.

This is the important reframe: the fully vendor-neutral integration point already exists and
costs one implementation.

### Layering

- **Universal — build once.** Pre-commit hook and CI action. Covers everyone, human or agent,
  indefinitely.
- **Portable-ish — build once.** `anchr init` detects the environment and writes
  AGENTS.md-compatible instructions plus MCP configuration where supported. `AGENTS.md` is the
  closest thing to a real cross-vendor standard, and Claude Code reads it as well.
- **Per-vendor — build on demand.** Native hooks give tighter feedback. Implement Claude Code
  first because that is the dogfooding environment. Add others when someone asks, not
  speculatively.

`init` is what keeps this from becoming N packages: one binary that wires up whatever it detects,
rather than a separate maintained artifact per vendor.

### Note on MCP

`DESIGN.md` defers MCP to v2 on the grounds that agents can shell out to the CLI. That reasoning
holds for **capability** but not for **distribution** — MCP is the only integration surface that
is vendor-neutral by construction.

Be precise about what it buys, though: MCP makes `check` *callable*, not *automatic*. It does not
replace a hook. Pull it forward only if cross-vendor reach is a v1 goal rather than a later one.

---

## 6. Decisions that affect implementation, not just release
<!-- @anchor[dist/implementation-decisions] -->

**Ship the LSP as a subcommand, not a second binary.** `anchr lsp` over stdio. One artifact,
half the distribution surface. rust-analyzer, biome, and ruff all do this.

**Tree-sitter grammars are the binary-size problem.** Each bundled grammar is compiled C.
Bundling forty pushes the binary into tens of megabytes, which is bad for `npx` and bad for a
hook running on every edit.

Recommendation: bundle a core set — TypeScript/JavaScript, Python, Rust, Go, plus markdown — and
treat the remainder as either a separate "full" build or dynamically loaded. Decide early;
unbundling later is painful.

This interacts directly with the **unverified** diagnostic class in @ref[#design/diagnostics]: "no
grammar for `.ex`" becomes a packaging decision rather than a permanent limitation, and the
diagnostic should be worded so that it points at the fix.

---

## 7. License

MIT, or the Rust-conventional Apache-2.0/MIT dual. For a tool whose value depends on ubiquity,
permissive is the only sensible choice.

---

## 8. Open questions

1. **Defensive reservation of `anchr`.** The command name is unclaimed on both registries but
   unprotected. Worth publishing a placeholder that points at `context-anchors`, or worth leaving
   alone as registry clutter?
2. **Is MCP a v1 requirement?** Depends entirely on whether cross-vendor reach is a launch goal
   or a follow-up. If launch, it moves up from `DESIGN.md` v2.
3. **Which grammars make the core bundle?** Driven by where the tool is actually used first.
4. **Does the CI action ship as a composite GitHub Action, or as documentation for calling the
   binary directly?** The action is friendlier; the documentation is portable to GitLab, Buildkite,
   and others.
