# TODO

Open work items, one line each. The reasoning behind a product deferral lives in the design docs;
this file only says what is not done yet.

## Release pipeline

- Staged npm publishing: CI runs `npm stage publish` and a maintainer approves each package with
  2FA. Needs @ref[scripts/src/npm/publish-packages.mjs] to stage, probe staged state, and promote
  shim-last, and a dist-tag answer for prereleases. Revisit at 1.0 with signing.
- Pin the actions in @ref[.github/workflows/publish-npm.yml] to commit SHAs instead of major tags.
- Release on a version bump instead of a hand-pushed tag: cargo-dist `dispatch-releases` in
  @ref[dist-workspace.toml] plus a job on push to `main` that dispatches when no release matches.
- Enable the Homebrew tap that cargo-dist already knows how to generate.
- Sign and notarise binaries. Trigger: 1.0, or the first Gatekeeper report, whichever comes first.
- Decide whether CI integration ships as a composite GitHub Action or as documentation for calling
  the binary directly.

## Product

- Signature review ledger: `anchr review` reports declaration-signature drift, `anchr accept`
  records a reviewer's sign-off. Non-blocking, separate from `check`.
- Exported vs. internal anchors, so a cross-root anchor is an explicit contract.
- MCP adapter, only if shelling out to the CLI proves insufficient for agents.
- VS Code extension; other editors already work through `anchr lsp`.
- A "full" grammar build or dynamic grammar loading beyond the bundled five languages.

## Code hygiene

- Bump cargo-dist to 0.33.0 (Azure signing for Windows; the shell installer moves its `env`
  script into the app's config directory). Rehearse with an rc tag before a real release.
