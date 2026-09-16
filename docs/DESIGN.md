---
title: Design
description: What anchr promises and the rules the code must never break, from the marker grammar to the three diagnostic classes and the five invariants. Read before changing what a marker means or what check reports.
tags: [design, markers, invariants]
---

# Design — structured references for unstructured prose

<!-- refs -->
@ref[#cli/check as Check]
@ref[#cli/coverage as Coverage]
@ref[#cli/rename as Rename]
@noref[x.md, foo.ts, .claude/]

## 1. Problem

Agentic coding runs on prose: `CLAUDE.md`, `AGENTS.md`, skills, READMEs, design docs, code
comments. That prose is dense with references to files, directories, declarations, and sections
of other documents, and nothing checks them. A function is renamed, a file moves, a section is
deleted, and every reference to it goes stale silently. A human who follows a dead reference
knows they hit a dead end; an agent searches, fails, and reports that the thing *does not exist*.
Code does not have this problem: a call to a renamed function does not compile, and the compiler
names every site. anchr brings that property to prose. Heuristic linters (ctxlint, drift) cover
adjacent ground at roughly 91% precision; the trade made here is **more authorship burden in
exchange for deterministic correctness with no LLM in the loop.**

## 2. Concept

An **anchor** marks a location and gives it a stable identity. A **reference** asserts that a
target resolves. One batch check reports every reference that does not. An anchor's identity is
deliberately separate from the prose around it: a heading can be reworded freely, and only
changing the id is a breaking change. Compiler semantics land on the identity, not on cosmetics.

## 3. Grammar
<!-- @anchor[design/grammar] -->

The marker language is regular: no nesting, no recursion, no precedence.

```text
@anchor[some-id]        declares an identity at this line
@ref[target]            asserts that target resolves
@ref[target as X]       the same, and names the target X within this file
@[X]                    a use of that name, checked through its declaration
@noref[a, b/, c.ts]     these strings are not references in this file (coverage only)

target      := [root ":"] body
body        := "#" anchor_id                  an @anchor with this id exists in the root
             | rel_path "#" symbol_name       a declaration with this name exists in that file
             | rel_path ["/"]                 the path exists; a trailing "/" requires a directory
root        := [A-Za-z0-9_-]+
anchor_id   := segment ("/" segment)*         segment := [A-Za-z0-9_] [A-Za-z0-9_.-]*
rel_path    := path_seg ("/" path_seg)*       root-relative
             | ("./" | ("../")+) path_seg ("/" path_seg)*   relative to the writing file
path_seg    := printable non-whitespace, excluding  [ ] # : \  and control chars
symbol_name := [A-Za-z_$] [A-Za-z0-9_$]*      unqualified: `Foo::bar` and `a.b` are rejected
alias       := [A-Za-z_] [A-Za-z0-9_]*        max 64 bytes, case-sensitive
noref entry := glob, no whitespace, neither @ nor `
```

Rules with their reasons:

- **`#` introduces a name; a bare target is a path.** Without it `@ref[docs]` is ambiguous
  between a directory and an id, and worse once ids contain `/`. Brackets on alias uses keep the
  same shape, so a bare `@word` (`@param`, a handle) is never a marker.
- **`@anchor`, not `@def`.** The static-analysis vocabulary is more precise and is jargon; these
  files are read by humans skimming GitHub at least as often as by tooling.
- **Bare paths are root-relative; `./` and `../` are file-relative.** A bare path means the same
  thing wherever it is written. The relative forms exist for directories that move as a unit
  (a skill, a package) and may climb, but never above the root. `./` is mandatory for a
  same-directory file so `x.md` never changes meaning with its location. The stored form is
  always root-relative; nothing downstream sees the spelling.
- **`:` after a root-shaped prefix and `#` anywhere are reserved.** A file whose name contains
  either cannot be referenced by path.
- **Symbols are unqualified** because the guarantee is file-scoped: *a declaration with this name
  exists in this file*. Matching `Foo::bar` on its last segment would make the check claim more
  than it knows.
- **Ids are unique per root**; a duplicate is an error. Ids may be hierarchical
  (`auth/token-refresh`), which is namespacing without reintroducing paths.
- **An anchor is a point, not a range.** It has no extent (§6).
- **Markers inside code fences, inline code, and comment backtick spans are not markers.** This
  is what lets documentation about anchr, this file included, contain example markers. A leading
  backslash (`\@ref[x]`) or a markdown escape (`@ref\[x\]`) also hides one. A plain `.txt` file
  has none of these, so plaintext cannot document the marker syntax; write such docs in markdown.
- **Aliases** borrow `import x as y` whole: file scope, undeclared use is an error, duplicate
  declaration is an error, the declaration is the one resolved site, unused is reported by
  @[Coverage] never by @[Check]. Full reasoning: @ref[docs/design/aliases.md].
- **Ignores** answer two questions with two keys, `[ignore] paths` ("look at this?") and
  `[ignore] tokens` plus the file-scoped `@noref` ("propose this?"). They remove coverage
  candidates and never make anything resolve. Full reasoning: @ref[docs/design/ignores.md].

The same markers work inside source-code comments; a comment pointing at a renamed function is
the same defect as a doc pointing at one.

## 4. Roots and resolution

A **root** is a namespace with a filesystem location. Bare references resolve in the current
root; `name:` prefixes resolve into a root declared in @ref[anchr.toml]. This is module
resolution: bare is local, prefixed is external. Roots exist because the motivating case spans
them: `~/.claude/` is not a repository, skills reference other skills across plugin boundaries, a
repository's `CLAUDE.md` may point into a globally installed skill. A repository's own `.claude/`
is scanned like any other directory; hidden is not a synonym for irrelevant. What should be
skipped is what `.gitignore` already says, applied exactly as git applies it, plus `[ignore]
paths` in the same syntax.

**A target exists only if the scan enumerated it.** The scan is a pure function of the
filesystem and the ignore rules, rerun on every invocation. A gitignored or config-ignored path
is on this machine and not in the repository, so it is not a target; a local run must agree with
a clean checkout. Lookups are exact, so `src/Foo.ts` never matches `foo.ts` on a case-insensitive
filesystem and then fails in CI.

Roots also give a **distinguishable error class**: "root `claude` is not present" and "reference
is broken" demand different responses, and conflating them reproduces the original failure of
being unable to tell *gone* from *not visible from here*.

## 5. Diagnostics
<!-- @anchor[design/diagnostics] -->

| Class | Meaning | Blocks |
|---|---|---|
| **error** | the target does not resolve | yes |
| **unverified** | the target could not be checked | no, but always reported |
| **clean** | the target resolved | — |

**Unverifiable must never render as valid.** A reference into a language with no grammar, into an
absent external root, into a file too large to scan, or into a parse tree with errors was not
checked, and a tool whose value is soundness cannot report "not checked" as "passing". Unverified
findings are visible and non-blocking; `--strict` promotes every one of them to an error, because
CI users read that flag as "fail on anything you could not check".

**Error reporting is the product.** Three requirements:

- **Group by cause, not by site.** Deleting an anchor with twelve live references is one error
  listing twelve locations. This is the property the whole idea chases: change the name, get
  pointed at every site still using the old one.
- **Suggest, never guess.** A did-you-mean by edit distance is safe because a suggestion never
  mutates anything and is not a correctness claim.
- **Two output modes.** Human text and JSON with a versioned schema. The JSON consumer is an
  agent or an editor, and structured output retrofitted onto string formatting is miserable.

## 6. Invariants
<!-- @anchor[design/invariants] -->

In priority order.

1. **@[Check] never writes.** A validation step that rewrites files under an agent desynchronises
   the agent's model of the file. Anything that does write announces what changed.
2. **Batch validation is the sole guarantee.** Edit by any means, then run one check that reports
   everything. Mutating commands (@[Rename], `annotate --write`) are conveniences and never
   load-bearing. This is the compiler model.
3. **Unverifiable never renders as valid.** §5.
4. **Opt-in only.** Nothing un-annotated is ever an error. Zero false positives on unmarked
   content is what keeps the tool enabled.
5. **Deterministic.** No LLM, no fuzzy matching in any error criterion. Fuzzy matching is
   permitted only for suggestions and for @[Coverage], neither of which can fail a build or
   mutate a file.

## 7. Deferred, with reasons
<!-- @anchor[design/deferred] -->

Tracked in @ref[TODO.md]; the reasoning lives here.

**Content signatures.** Resolution has a blind spot: `validateToken` still exists, but the
paragraph describing it is now wrong. Hashing the target would catch that, and it is deferred
because it is a categorically different mechanism. Resolution is binary and objective; a hash
cannot tell a variable rename from a semantic inversion, so it produces suspicion, not a
finding. Letting it into @[Check] dilutes a hard guarantee with a soft signal and ends in
`--no-verify`. When built it is a separate non-blocking command pair: `review` reports drift,
`accept` records that a reviewer asserted the prose is accurate as of hash X. The interesting
product is "when was this documentation last verified against the code", which nothing provides.
Two constraints: hash the declaration signature, not the body, since prose describes contracts and
body hashes nag on every refactor; and make the ledger line-oriented and sorted, because it is
the only committed file in the design and lockfile merge conflicts are how tools become hated.

**Extent, dropped.** An anchor was going to carry a range so signatures had something to hash.
With signatures deferred nothing needs it; the container AST is still parsed for fence exclusion,
but block-boundary reasoning is gone.

**Exported vs. internal anchors.** A cross-root anchor is a published contract: deleting it
breaks a downstream repository while the upstream's own CI stays green. The compiler answer is
`pub`/`export`. Deferred until a real plugin ecosystem forms; single namespace per root ships
first.

**MCP adapter.** Agents can shell out to the CLI exactly as a human would. MCP makes @[Check]
*callable*, not *automatic*, and does not replace a hook; it is worth building only if the CLI
proves insufficient. A git pre-commit hook and CI already catch every broken reference regardless
of what did the editing, which is the vendor-neutral integration point.

## Rejected alternatives

- `@def`/`@ref` over `@anchor`/`@ref`: precise, symmetric, and jargon to the GitHub reader.
- A bare `@Alias` sigil over `@[Alias]`: JSDoc tags and `@handle` mentions would all be errors.
- Root-then-file fallback for bare paths over explicit `./`: a target string would stop having
  one meaning, and check results would change when an unrelated file appeared.
- Matching qualified symbols on their last segment over rejecting them: a guaranteed miss with a
  useless suggestion, and a claim the file-scoped guarantee cannot back.
- Heuristic detection over opt-in markers: 91% precision means false positives, and false
  positives are what get linters disabled.
