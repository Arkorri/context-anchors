# context-anchors — structured references for unstructured prose

**Status:** design draft
**Package / repo:** `context-anchors`
**Command:** `anchr`
**Markers:** `@anchor` / `@ref`
<!-- refs -->
@ref[#cli/check as Check]
@ref[#cli/coverage as Coverage]
@ref[#cli/backrefs as Backrefs]
@ref[#cli/rename as Rename]

---

## 1. Problem

Agentic coding environments run on unstructured prose: `CLAUDE.md`, `AGENTS.md`, skills,
READMEs, design docs, and code comments. That prose is dense with references — file paths,
directory paths, function and type names, and pointers to sections of other documents.

None of those references are checked by anything. They go stale silently during ordinary
iteration: a function is renamed, a file is moved, a document section is retitled or deleted,
and the references pointing at it are not updated. The breakage surfaces much later, when a
reader — human or agent — tries to follow the reference and cannot.

The failure is worse for agents than for humans. A human who follows a dead reference knows
they hit a dead end. An agent will often search for the missing target, fail to find it, and
then report that the thing *does not exist* — when in fact it exists under a different name or
in a different place, and only the search failed. A confidently wrong "not found" is more
damaging than an obvious break.

Code does not have this problem. A reference to a renamed function does not compile, and the
compiler names every site that still uses the old name. The goal here is to bring that property
to prose.

---

## 2. Prior art, and what is different

Two existing tools cover adjacent ground.

**[fiberplane/drift](https://github.com/fiberplane/drift)** binds markdown docs to code with
inline anchors (`@./src/auth/provider.ts#AuthConfig`), parses targets with tree-sitter, stores
AST fingerprints in a lockfile, and ships an agent skill. It covers doc→code references well.
It does not cover references between documents, and its anchors are path-qualified, so they
break when a file moves.

**[ctxlint](https://github.com/YawLabs/ctxlint)** and **[agents-lint](https://github.com/giacomo/agents-lint)**
scan `CLAUDE.md`/`AGENTS.md` heuristically for paths and commands and validate them against the
repo, with fuzzy auto-fix from git history. Reported precision is roughly 91%.

Three gaps motivate building rather than adopting:

1. **Document→document section references are unhandled by everything.** In a skills and
   context-file ecosystem this is the most common reference type — one skill pointing at a
   section of another, `CLAUDE.md` pointing at a section of a design doc.
2. **Heuristic detection cannot be sound.** 91% precision means false positives, and false
   positives are what get linters disabled. An opt-in annotation is 100% accurate on what it
   covers and silent on everything else.
3. **One tool, one grammar.** Paths, code symbols, and document sections are the same problem.
   Needing separate tools with separate syntaxes for them is a worse product than a single
   package, even at the cost of higher authorship.

The explicit trade being made: **more authorship burden, in exchange for deterministic
correctness with no LLM in the loop.**

---

## 3. Core concept

Create durable, checkable pointers into unstructured prose.

An **anchor** (`@anchor`) marks a location and gives it a stable identity. A **reference**
(`@ref`) asserts that some target resolves. A single batch validation step reports every
reference that does not.

The identity of an `@anchor` is deliberately separate from the prose around it. A heading's text
can be reworded freely; only changing the `@anchor` ID itself is a breaking change. This gives compiler
semantics exactly where they belong — on the identity — without making cosmetic edits expensive.

---

## 4. Grammar
<!-- @anchor[design/grammar] -->

The entire marker language is four productions.

```
@anchor[some-id]     declares an identity at this location
@ref[target]         asserts that target resolves
@ref[target as X]    the same, and names the target X within this file
@[X]                 a use of that name, checked through its declaration
@noref[a, b/, c.ts]  declares that these strings are not references in this file
```

**Why `@anchor` rather than `@def`.** The static-analysis pair `@def`/`@ref` is more precise and
more symmetric, and it is the vocabulary SCIP and LSIF use. It is also jargon. `@anchor` explains
itself to a reader who has never seen this tool — it is already the word HTML and markdown use
for a named location in a document. These files are read by humans skimming them on GitHub at
least as often as by tooling, and for that audience self-explanatory beats precise.

Note that references will vastly outnumber anchors in practice: paths and symbols get referenced
constantly, while anchors are declared only where something needs to be pointed at.

### `@anchor`

Placed on the line it names. Inherits the file path of the file it lives in, so the reference
site never needs to spell out a path.

```markdown
### How references are written    @anchor[ref-grammar]
```

- IDs are **unique per root**. A duplicate ID is an error.
- IDs may be **hierarchical**: `@anchor[auth/token-refresh]`. This is namespacing without
  reintroducing paths, and it self-documents at the reference site.
- Multiple `@anchor` markers on one line are permitted. Expected to be rare.
- An `@anchor` is a **named point**, not a range. It has no extent. (See §9.)

Because the marker lives in the file, it moves with its content. There are no stored line
numbers to invalidate, and moving or renaming the containing file breaks nothing.

### `@ref`

One operator, one target grammar, five resolution kinds:

```
@ref[src/directory]                  directory exists
@ref[src/file.ts]                    file exists
@ref[src/file.ts#FunctionName]       named declaration exists in that file
@ref[#auth/token-refresh]            @anchor with this ID exists in this root
@ref[claude:#auth/token-refresh]     @anchor with this ID exists in a declared external root
```

**`#` introduces a name; a bare target is a path.** This is the disambiguation rule. Without it
`@ref[docs]` is ambiguous between a directory named `docs` and an ID named `docs`, and the
ambiguity gets worse once hierarchical IDs contain `/`. With it, the grammar needs no lookahead
and IDs can never collide with paths. The same reasoning keeps brackets on alias uses: `@[X]`
has the shape of every other marker, so a bare `@word` in prose or a comment (`@param`, a
handle) is never one.

### Aliases

A qualified reference is long, and a document that names the same thing ten times will not
write it ten times. Programming languages settled this with `import x as y`, and the semantics
are borrowed whole: a file declares a target once under a local name and uses the name at every
mention.

```markdown
<!-- refs -->
@ref[src/auth/session.ts#refreshToken as RefreshToken]
@ref[#auth/token-refresh as TokenRefresh]

@[RefreshToken] implements the state machine in @[TokenRefresh].
```

- Scope is the **file**. The same alias in two files is two unrelated declarations.
- A use with no declaration in its file is an **error**, with a did-you-mean over the file's
  aliases. Two declarations of one alias in a file is an error. An unused alias is reported by
  @[Coverage], never by @[Check].
- The declaration is the reference: it is resolved, and a broken target reports there, once. Uses
  bind to it and are not re-resolved. Renaming the target in code is one edit per file.
- Aliases are ASCII identifiers. `@anchor` is never aliased; an anchor is the identity of a line.

The full design, with the reasons behind each rule, is @ref[docs/design/aliases.md].

### Ignores

@[Coverage] reports reference-shaped strings that carry no marker. Some of them are correctly
shaped and still not references: an example path in a guide, a file that exists in the reader's
repository rather than this one, the product's own name. The author says so once, and the
report stops asking.

```markdown
<!-- refs -->
@noref[src/legacy/, foo.ts]
```

```toml
[coverage]
exclude = ["docs/research/**"]      # still checked; never asked for more annotation
ignore  = ["CLAUDE.md", "AGENTS.md"] # never a reference anywhere in this root
```

- `@noref` is **file-scoped**, like an alias: the ignore travels with the text it protects.
  Config `ignore` is root-wide. Matching is exact, plus the path of a `path#Symbol` token and a
  trailing-`/` prefix; no globs.
- An entry that matches nothing is reported by @[Coverage], the way an unused alias is. Ignore
  lists rot otherwise.
- @[Check] is untouched. Ignores remove coverage candidates; they never make anything resolve.

The full design is @ref[docs/design/ignores.md].

### Markers in code

The same markers work inside source-code comments. Code comments go stale in exactly the same
way prose does, and a comment pointing at a renamed function is the same defect as a doc
pointing at one.

```typescript
// Mirrors the state machine described in @ref[#auth/token-refresh]
```

---

## 5. Resolution and roots

A **root** is a namespace with a filesystem location, declared in config. Bare references
resolve within the current root; prefixed references resolve into a declared external root.

```
@ref[#some-id]           this root
@ref[claude:#some-id]    the root declared as `claude`
```

This is module resolution, and it is deliberately the same mental model as package imports —
bare is local, prefixed is external.

Roots exist because the motivating use case spans them. `~/.claude/` is not a git repository,
skills reference other skills across plugin boundaries, and a repository's `CLAUDE.md` may point
into a globally installed skill. Every lockfile-at-repo-root assumption breaks on that case.

Roots also give a **distinguishable error class**. "Root `claude` is not present" and "reference
is broken" demand entirely different responses, and conflating them reproduces the original
failure — being unable to tell *gone* from *not visible from here*.

---

## 6. Diagnostics
<!-- @anchor[design/diagnostics] -->

Validation produces three classes. The third is the one most tools get wrong.

| Class | Meaning | Blocks |
|---|---|---|
| **error** | Target does not resolve | yes |
| **unverified** | Target could not be checked | no, but always reported |
| **clean** | Target resolved | — |

**Unverifiable must never render as valid.** If there is no tree-sitter grammar for `.ex`, then
`@ref[src/thing.ex#SomeFunction]` was not checked — reporting it as passing is a silent false
negative in a tool whose entire value is soundness. It gets its own visible class:

```
12 refs unverified: no grammar for .ex
 3 refs unverified: root `claude` not present
```

An absent external root falls into this same class. One concept, applied consistently.

### Error reporting is the product

If a single batch @[Check] is the whole guarantee, then the error report *is* the user
experience. Three requirements, not polish:

**Group by cause, not by site.** Deleting `@anchor[auth/flow]` with 12 live references is *one*
error with 12 locations, not 12 errors. This is the property the whole idea is chasing: change
the name, get pointed at every site still using the old one.

```
error: unknown id `auth/flow`
  referenced from 12 locations:
    CLAUDE.md:14
    docs/architecture.md:88
    src/auth/provider.ts:31   (comment)
    ...
```

**Suggest, but never guess.** `did you mean 'auth/token-refresh'?` via edit distance. A
suggestion is not a correctness claim and never mutates anything, so fuzzy matching is safe
here even though it is banned from the error criterion itself.

**Two output modes from day one.** Human text and JSON. The JSON consumer is an agent or an LSP
client, and retrofitting structured output onto string formatting is miserable.

---

## 7. Architecture
<!-- @anchor[design/architecture] -->

### Pipeline

Three container parsers, one marker lexer, one index.

```
container parse  →  extract text regions  →  lex markers  →  index  →  resolve  →  diagnostics
```

**Container parsing** uses existing parsers. Never write a markdown parser.

| Container | Parser | Marker locations |
|---|---|---|
| Markdown | CommonMark (comrak) | text nodes |
| Source code | tree-sitter | comment nodes |
| Plain text | none | whole file |

This is load-bearing, not convenience: **markers inside fenced code blocks must not be
checked.** Documentation about `anchr` will contain example `@ref[...]` markers, and a naive
regex sweep over raw text will check and fail them. Only the container AST distinguishes prose
from a fenced example.

**Marker lexing** does not need an AST. `@anchor[...]` / `@ref[...]` is a regular language — no
nesting, no recursion, no precedence. A lexer over the container's text regions is sufficient
and considerably faster than a parser generator. A tree-sitter grammar for the marker language
would be over-engineering.

**Target resolution** for `#Symbol` uses tree-sitter against the referenced source file. Scope
of the guarantee: *a declaration with this name exists in this file*. Overloads, methods needing
a nesting path, re-exports, generated code, and conditional compilation are beyond it. That
scope covers the overwhelming majority of references in prose; claiming more would be dishonest.

### Index

Maps ID → (root, path, line). Purely derived, incremental, regenerable from scratch,
**gitignored**. Never authoritative.

The format must remain resolvable by `grep` alone — `grep '@anchor\[some-id\]'` finds the
definition in one hop. The index makes resolution fast; it is never required for correctness.
This keeps the tool honest and keeps the files useful when the tool is absent.

### The core is LSP-shaped

The query surface maps onto LSP exactly, which means one index serves every consumer:

| LSP | anchr |
|---|---|
| diagnostics | @[Check] |
| go-to-definition | follow a ref |
| find-references | @[Backrefs] |
| rename | refactor an `@anchor` ID |
| document symbols | anchors in a file |

Ship an LSP server and every editor gets squiggles, jump-to-ref, and rename refactoring with no
per-editor work. CLI, MCP, and CI are then thin adapters over the identical index. The human
story and the agent story stop competing for the same effort.

### Implementation

Rust. tree-sitter's canonical bindings are Rust, comrak covers CommonMark, tower-lsp covers the
server — but the deciding factor is **single static binary**. This has to run in a pre-commit
hook on a machine nobody controls, and requiring a language runtime first is a real adoption
tax. Go is a legitimate alternative and would not be a mistake.

Distribute prebuilt binaries via curl, homebrew, and an npx wrapper.

---

## 8. Invariants
<!-- @anchor[design/invariants] -->

Design rules committed to, in priority order.

1. **@[Check] never writes.** Fix is always a separate command invoked deliberately. A validation
   step that rewrites files underneath an agent desynchronizes the agent's model of the file and
   produces either a rejected edit or, worse, a successful edit against a stale mental model. If
   any integration does auto-apply fixes, it must announce what changed and which files to
   re-read.

2. **Batch validation is the sole guarantee.** Edit freely by any means — the tool, an editor,
   an agent, `sed` — then run one validation that reports everything at once. Mutating commands
   (@[Rename], `rm`, `fix`) are convenience only and are never load-bearing. Guarding the commands
   *and* validating globally would be redundant, and the guards would be trivially bypassed
   anyway. This is the compiler model: modify however you like, get all errors in one batch.

3. **Unverifiable never renders as valid.** See §6.

4. **Opt-in only.** Nothing un-annotated is ever an error. Zero false positives on unmarked
   content is the property that keeps the tool enabled.

5. **Deterministic.** No LLM, no fuzzy matching in any error criterion. Fuzzy matching is
   permitted only for suggestions and for the coverage scanner, neither of which can fail a
   build or mutate a file.

---

## 9. Deferred, with reasoning
<!-- @anchor[design/deferred] -->

### Content signatures — deferred to v2

Resolution checking has one blind spot: a reference can resolve while the prose describing the
target has become wrong. `validateToken` still exists, but it now also checks expiry and returns
a `Result`, and the paragraph describing it is silently false.

Hashing the target and flagging changes would catch this. It is deferred because **it is a
categorically different mechanism from everything else here.** Resolution is binary and
objective. A content hash cannot distinguish a variable rename inside a function body from a
semantic inversion — it produces suspicion, not a finding. Letting that into @[Check] dilutes a
hard guarantee with a soft signal, and the predictable result is people disagreeing with
failures and reaching for `--no-verify`.

When it is built, it belongs behind a separate non-blocking command:

```
anchr check     resolution only.   errors.    exit 1.  blocks.
anchr review    signature drift.   findings.  exit 0.  never blocks.
```

The more interesting framing is that the hash is not the point — the **accept** is. `anchr
accept <target>` records that a reviewer asserted this prose is accurate as of hash X, and the
hash exists only to invalidate that assertion later. What that actually builds is *when was this
documentation last verified against the code*, which nothing currently provides.

Two constraints for whenever it lands: hash the **declaration signature** (name, params, types)
rather than the full body, since prose overwhelmingly describes contracts and full-body hashing
nags on every internal refactor until people stop reading it; and make the signature lock
line-oriented and sorted, because it is the only file in this design that must be committed and
lockfile merge conflicts are how tools like this become hated.

### Extent — dropped

An `@anchor` was going to carry a range (a heading's section, a paragraph) so signatures had
something to hash. With signatures deferred, nothing needs it. An `@anchor` is a named point. The
container AST is still parsed — it is required for code-fence exclusion — but block-boundary
reasoning is gone entirely.

### Exported vs. internal anchors — deferred to v2

Because deleting an `@anchor` with live references is an error, cross-root anchors are effectively a
published contract: a plugin deleting an anchor breaks a downstream repository while the
plugin's own CI stays green. The compiler answer transfers directly — `pub`/`export`, where only
exported anchors are cross-root referenceable and only exported anchors carry the deletion guarantee.

Deferred because it is not needed until a real plugin ecosystem forms around this. Ship
single-namespace-per-root first.

### MCP adapter — deferred

Agents can shell out to the CLI exactly as a human would. An MCP server is worth building only
if the CLI proves insufficient in practice.

---

## 10. Scope
<!-- @anchor[design/scope] -->

### v1 — the guarantee

- Marker lexer over three container types: CommonMark, tree-sitter comment nodes, plaintext
- Five resolution kinds: directory, file, `file#Symbol`, `#id`, `root:#id`
- Per-root duplicate-ID detection
- Derived index — incremental, gitignored
- `anchr check` — batch, cause-grouped, three diagnostic classes, human + JSON output
- Roots config
- Single static binary: curl, brew, npx wrapper

**Agent integration in v1 is configuration, not code**: a `PostToolUse` hook that runs
`anchr check`, plus a skill teaching the annotation grammar. No agent-specific code paths. The
motivating use case ships in v1 for near-zero marginal work.

### v1.1 — the multiplier

- **LSP server.** Squiggles, go-to-definition, find-references, and rename in every editor, from
  the same index. Higher priority than anything in v2 — it is what makes "a compiler for docs"
  literally true for humans, with no agent involved.
- `anchr coverage` / `anchr annotate` — the demoted heuristic scanner. Opt-in has a blind spot
  symmetrical to the false-positive one: a document can be fully green because nobody annotated
  anything. Determinism gives soundness; it gives nothing on coverage. The scanner reports
  "43 of 210 reference-shaped strings are annotated" and proposes annotations. It never errors
  and never writes on its own. This is also the migration path onto the tool. `@noref` and
  `[coverage] ignore` let the author retire the candidates that are correctly shaped and still
  not references, so the report can reach zero.

- `anchr rename`, `anchr backrefs`

### v2

- Signature review ledger (`review` / `accept`)
- Exported vs. internal anchors
- MCP adapter, if warranted

---

## 11. Open questions
<!-- @anchor[design/open-questions] -->

1. **Do directory references earn their place?** Directories rarely vanish without their files
   vanishing too. Nearly free to implement, so currently a "why not" rather than a real question.
2. **Unknown-root default.** Currently classed as *unverified*. Should it be configurable to a
   hard error for CI environments that expect all roots present?
3. **Example markers in plaintext containers.** Markdown has code fences, so examples can be
   excluded structurally, and source files have comment nodes. A `.txt` file has neither — there
   is no way to distinguish a live reference from one being shown as an example. Options: an
   escape form, an ignore directive, or accepting that plaintext containers cannot carry
   documentation about the marker syntax itself.
4. **Validation of the premise.** Before writing code: run `ctxlint` and `drift` across real
   repositories and `~/.claude` and count actual broken references. That establishes the base
   rate for free. A large count justifies building; a small one means the problem feels bigger
   than it is because each instance is memorably annoying.
