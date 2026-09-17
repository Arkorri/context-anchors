import assert from "node:assert/strict";
import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";

import {
  MDBOOK_VERSION,
  PAGES,
  REPOSITORY_URL,
  buildSite,
  collectAnchors,
  loadPages,
  renderSummary,
  rewritePage,
  stripFrontmatter,
} from "../../src/site/build.mjs";

const PAGE = { name: "page.md", title: "Page" };
const OTHER = { name: "other.md", title: "The other page" };

function ctx(anchors = new Map()) {
  return { page: PAGE, pages: [PAGE, OTHER], anchors };
}

test("frontmatter is removed and a file without one is untouched", () => {
  assert.equal(stripFrontmatter("---\ntitle: T\ndescription: D\n---\n\n# T\n"), "\n# T\n");
  assert.equal(stripFrontmatter("# No block\n"), "# No block\n");
  assert.equal(stripFrontmatter("---\nunclosed\n"), "---\nunclosed\n");
});

test("the summary lists the pages in order with their titles", () => {
  assert.equal(
    renderSummary([PAGE, OTHER]),
    "# Summary\n\n- [Page](page.md)\n- [The other page](other.md)\n",
  );
});

test("a bare reference becomes a GitHub link, a directory to the tree and a file to the blob", () => {
  assert.equal(
    rewritePage("See @ref[src/parse.rs] and @ref[src/dir/].", ctx()),
    `See [\`src/parse.rs\`](${REPOSITORY_URL}/blob/main/src/parse.rs) and [\`src/dir/\`](${REPOSITORY_URL}/tree/main/src/dir).`,
  );
});

test("a symbol reference links to its file", () => {
  assert.equal(
    rewritePage("@ref[src/parse.rs#parse_file]", ctx()),
    `[\`src/parse.rs#parse_file\`](${REPOSITORY_URL}/blob/main/src/parse.rs)`,
  );
});

test("a reference to a guide page becomes a relative link titled from its frontmatter", () => {
  assert.equal(rewritePage("Read @ref[docs/guide/other.md].", ctx()), "Read [The other page](other.md).");
  assert.equal(rewritePage("Read @ref[./other.md].", ctx()), "Read [The other page](other.md).");
});

// The refs block is an implementation detail of checking; the reader sees links at each use.
test("the refs block is dropped and every alias use becomes a link", () => {
  const text = [
    "# Page",
    "",
    "<!-- refs -->",
    "@ref[docs/guide/other.md as Other]",
    "@ref[src/parse.rs as Parser]",
    "@noref[a.md, b/]",
    "",
    "",
    "See @[Other] and @[Parser].",
  ].join("\n");
  assert.equal(
    rewritePage(text, ctx()),
    `# Page\n\nSee [The other page](other.md) and [Parser](${REPOSITORY_URL}/blob/main/src/parse.rs).`,
  );
});

test("an inline declaration links at the declaration and at each later use", () => {
  assert.equal(
    rewritePage("First @ref[src/parse.rs as Parser], then @[Parser].", ctx()),
    `First [Parser](${REPOSITORY_URL}/blob/main/src/parse.rs), then [Parser](${REPOSITORY_URL}/blob/main/src/parse.rs).`,
  );
});

test("anchors become invisible HTML anchors and anchor references link to them", () => {
  const pages = [
    { ...PAGE, text: "## Targets\n<!-- @anchor[guide/targets] -->\n" },
    { ...OTHER, text: "Inline @anchor[guide/other] here.\n" },
  ];
  const anchors = collectAnchors(pages);
  assert.deepEqual([...anchors], [["guide/targets", "page.md"], ["guide/other", "other.md"]]);
  assert.equal(
    rewritePage(pages[0].text, { page: PAGE, pages, anchors }),
    '## Targets\n<a id="guide/targets"></a>\n',
  );
  assert.equal(
    rewritePage("Same @ref[#guide/targets], other @ref[#guide/other].", { page: PAGE, pages, anchors }),
    "Same [`#guide/targets`](#guide/targets), other [`#guide/other`](other.md#guide/other).",
  );
  assert.equal(
    rewritePage(pages[1].text, { page: OTHER, pages, anchors }),
    'Inline <a id="guide/other"></a> here.\n',
  );
});

test("a duplicate anchor is an error naming both pages", () => {
  const pages = [
    { ...PAGE, text: "<!-- @anchor[dup] -->\n" },
    { ...OTHER, text: "<!-- @anchor[dup] -->\n" },
  ];
  assert.throws(() => collectAnchors(pages), /other\.md: anchor `dup` is already declared in page\.md/);
});

// Examples must survive untouched, or the site would document a syntax it does not show.
test("markers inside code fences and inline code are left alone", () => {
  const text = [
    "Write `@ref[src/parse.rs]` or ``@[Name]``:",
    "",
    "```markdown",
    "<!-- refs -->",
    "@ref[src/parse.rs as Parser]",
    "@[Parser] and @anchor[x]",
    "```",
    "",
    "~~~",
    "@ref[#missing]",
    "~~~",
    "",
    "Escaped: \\@ref[x] and @ref\\[x\\].",
  ].join("\n");
  assert.equal(rewritePage(text, ctx()), text);
});

test("a noref comment is dropped and an inline noref disappears", () => {
  assert.equal(rewritePage("<!-- @noref[a, b/] -->\nText @noref[c] here.", ctx()), "Text  here.");
});

test("an unknown alias, an anchor outside the guide, a second declaration, and an external root are errors", () => {
  assert.throws(() => rewritePage("@[Nope]", ctx()), /page\.md: `@\[Nope\]` is used before any declaration/);
  assert.throws(() => rewritePage("@ref[#elsewhere]", ctx()), /page\.md: `@ref\[#elsewhere\]` names an anchor not declared in the guide/);
  assert.throws(
    () => rewritePage("@ref[a.md as A]\n@ref[b.md as A]", ctx()),
    /page\.md: alias `A` is declared twice/,
  );
  assert.throws(() => rewritePage("@ref[specs:#x]", ctx()), /points into another root/);
});

test("runs of blank lines collapse to one", () => {
  assert.equal(rewritePage("a\n\n\n\nb\n\n", ctx()), "a\n\nb\n");
});

test("loading the guide directory refuses a page that is not in PAGES and a PAGES entry with no file", () => {
  const dir = mkdtempSync(join(tmpdir(), "site-"));
  for (const name of PAGES) {
    writeFileSync(join(dir, name), `---\ntitle: ${name}\ndescription: D\n---\n# ${name}\n`);
  }
  assert.deepEqual(loadPages(dir).map((page) => page.name), PAGES);
  writeFileSync(join(dir, "extra.md"), "---\ntitle: X\ndescription: D\n---\n");
  assert.throws(() => loadPages(dir), /unlisted \["extra\.md"\]/);
});

test("the site is the summary plus one rewritten file per page", () => {
  const pages = [
    { ...PAGE, text: "---\ntitle: Page\ndescription: D\n---\n\n# Page\n\nSee @ref[docs/guide/other.md].\n" },
    { ...OTHER, text: "---\ntitle: The other page\ndescription: D\n---\n\n# Other\n" },
  ];
  const site = buildSite(pages);
  assert.deepEqual([...site.keys()], ["SUMMARY.md", "page.md", "other.md"]);
  assert.equal(site.get("page.md"), "\n# Page\n\nSee [The other page](other.md).\n");
});

test("the pinned mdBook version is a release number", () => {
  assert.match(MDBOOK_VERSION, /^\d+\.\d+\.\d+$/);
});
