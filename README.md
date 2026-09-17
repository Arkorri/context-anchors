# context-anchors

[![CI](https://github.com/Arkorri/context-anchors/actions/workflows/ci.yml/badge.svg)](https://github.com/Arkorri/context-anchors/actions/workflows/ci.yml)
[![npm](https://img.shields.io/npm/v/context-anchors)](https://www.npmjs.com/package/context-anchors)
[![Homebrew](https://img.shields.io/badge/dynamic/regex?url=https%3A%2F%2Fraw.githubusercontent.com%2FArkorri%2Fhomebrew-tap%2Fmain%2FFormula%2Fcontext-anchors.rb&search=version%20%22(%5B%5E%22%5D%2B)%22&replace=%241&label=homebrew)](https://github.com/Arkorri/homebrew-tap)

**Your docs point at your code. `anchr` tells you when they stop matching.**

Design docs, READMEs, and code comments are full of references to files, functions, and sections
of other documents. Rename a function, and every mention of it in prose quietly goes stale.
`anchr` makes those references checkable: mark one, and it fails the day it stops resolving, the
same way a compiler catches a call to a function you deleted.

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

One rename with twelve live references is a single error listing twelve places to fix.

## Install

```sh
npm install --save-dev context-anchors      # pinned in your lockfile; run it as `npx context-anchors`
brew install Arkorri/tap/context-anchors    # macOS, Linux
```

Or the installer script, which puts a standalone `anchr` on your `PATH` with no Node involved:

```sh
# macOS, Linux
curl --proto '=https' --tlsv1.2 -LsSf \
  https://github.com/Arkorri/context-anchors/releases/latest/download/context-anchors-installer.sh | sh

# Windows
powershell -ExecutionPolicy Bypass -c "irm https://github.com/Arkorri/context-anchors/releases/latest/download/context-anchors-installer.ps1 | iex"
```

Binaries for every platform are attached to each GitHub release, and
`cargo install --git https://github.com/Arkorri/context-anchors context-anchors` builds from source.

## Quick start

```sh
anchr init               # writes anchr.toml and a one-page marker guide for coding agents
anchr coverage           # lists strings that look like references but are not yet checked
anchr annotate --write   # marks the ones that resolve
anchr check              # exit 0 clean, 1 broken references, 2 something went wrong
```

A marked reference reads the same as before:

```markdown
The parser lives in @ref[src/parse.rs].
```

Delete or move that file and `anchr check` fails, naming every document that mentioned it.

## In CI

Run it like a linter. With the npm package in your lockfile:

```yaml
- run: npm ci
- run: npx context-anchors check --strict
```

Or with the GitHub action, which installs the release its tag names and puts each finding on the
pull-request diff:

```yaml
- uses: actions/checkout@v7
- uses: Arkorri/context-anchors@v0.0.4
```

## Learn more

The user guide at <https://arkorri.github.io/context-anchors/> covers every marker, command, and
setting, the CI setups, and editor and coding-agent integration. Developer documentation starts
at @ref[docs/README.md].

## License

MIT or Apache-2.0, at your option: @ref[LICENSE-MIT] and @ref[LICENSE-APACHE].
