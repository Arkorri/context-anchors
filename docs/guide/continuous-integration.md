---
title: Continuous integration
description: Fail the build on a broken reference, with npm, the GitHub Action, or the installer script, and which flags to pass.
tags: [guide, ci, github-actions]
---

# Continuous integration

<!-- refs -->
@ref[docs/guide/commands.md as CommandsPage]
@ref[docs/guide/configuration.md as ConfigurationPage]
@noref[anchr.toml, docs/]

`anchr check` exits 0 when every reference resolves and 1 when one does not, so it runs in CI
like a linter: one step, and a broken reference fails the build. There are three ways to get the
binary onto the runner. Pick the one that matches how your project already installs tools.

## In a project that installs npm packages

If `context-anchors` is a dev dependency, the lockfile pins its version and one step runs it:

```yaml
- uses: actions/setup-node@v7
  with:
    node-version: 24
- run: npm ci
- run: npx context-anchors check --strict
```

Use `npm ci` rather than `npm install`. Rebuilding a lockfile on top of an existing
`node_modules` can record only your own platform's binary, leaving other machines without one.
If that happens, `anchr` says which package is missing.

## With the GitHub Action
<!-- @anchor[guide/action] -->

On GitHub without Node, the action installs a release and runs it. Findings appear as
annotations on the pull request diff:

```yaml
- uses: actions/checkout@v7
- uses: Arkorri/context-anchors@v0.0.4
```

The tag you pin is the release you get: `@v0.0.4` installs 0.0.4. The action runs on Linux,
macOS, and Windows runners, and by default runs `check --strict --format github` in the checkout.

| Input | Default | Meaning |
|---|---|---|
| `version` | the tag the action was called with | a release tag such as `v0.0.4`, or `latest` |
| `args` | `check --strict --format github` | the arguments passed to `anchr` |
| `working-directory` | `.` | where to run; the root is found from there |

For example, to check without `--strict`:

```yaml
- uses: Arkorri/context-anchors@v0.0.4
  with:
    args: check --format github
```

## Anywhere else

The installer script works in any CI job. It puts `anchr` in `~/.cargo/bin`, which the same
step can use directly:

```sh
curl --proto '=https' --tlsv1.2 -LsSf \
  https://github.com/Arkorri/context-anchors/releases/latest/download/context-anchors-installer.sh | sh
"$HOME/.cargo/bin/anchr" check --strict
```

To pin a version, replace `latest/download` in the URL with `download/v0.0.4`.

## Choosing the flags

**`--strict`** fails the run on anything anchr could not verify, as well as on broken
references. Use it when every reference in your project can be checked. Leave it off if you
reference declarations in a language anchr cannot parse; those references are then listed as
unverified in the log without failing the build. The same choice can be made once in
`anchr.toml` instead (@[ConfigurationPage]).

**`--format github`** adds one annotation per finding after the normal report, so each broken
reference shows on the pull request diff at the line that wrote it. It works on any GitHub
runner, with or without the action.

**Paths** narrow the report. `anchr check docs/` reports only references written under `docs/`.
The whole root is still read, because a reference in `docs/` can point anywhere.

Exit codes and the full option list are in @[CommandsPage].
