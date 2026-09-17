#!/usr/bin/env node
// Assembles the mdBook source for the documentation site from the user guide. The guide is
// written with anchr's own markers so that this repository checks it; a reader of the site
// should see links, not markers. So each page is copied with its frontmatter removed and every
// marker rewritten:
//
//   `@ref[docs/guide/x.md]`               a link to that page, titled from its frontmatter
//   `@ref[path]`, `@ref[path#Name]`       a link to the file on GitHub
//   `@ref[#id]`                           a link to the page and anchor that declares `id`
//   `@ref[t as Name]` + `@[Name]`         the declaration is dropped, each use becomes a link
//   `@anchor[id]`, `<!-- @anchor[id] -->`  an invisible HTML anchor with that id
//   `@noref[...]`, `<!-- refs -->`        dropped
//
// Markers inside code fences and inline code are examples and are left alone, exactly as
// `anchr check` leaves them alone. A marker the site cannot link (an unknown alias, an anchor
// declared outside the guide, an external root) is an error, so the build fails on the pull
// request rather than publishing a dead link.
//
//   node scripts/src/site/build.mjs [--guide <dir>] [--out <dir>]     write <out>/SUMMARY.md and the pages
//   node scripts/src/site/build.mjs --mdbook-version                    print the mdBook version to install
//
// Then `mdbook build site` renders <out>'s parent. The pinned mdBook version lives here so both
// workflows install the same one.
//
// @noref[SUMMARY.md, scripts/src/site/build.mjs]

import { mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { join, posix, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";

import { collectDocs } from "../docs/gen-index.mjs";
import { repoRoot } from "../repo-root.mjs";

export const MDBOOK_VERSION = "0.5.4";
export const REPOSITORY_URL = "https://github.com/Arkorri/context-anchors";
export const GUIDE_DIR = "docs/guide";

/// The pages in sidebar order. Every markdown file in the guide directory must be listed here
/// and vice versa, so a page is never silently missing from the site.
export const PAGES = [
  "getting-started.md",
  "markers.md",
  "commands.md",
  "configuration.md",
  "continuous-integration.md",
  "editors.md",
];

const REPO_ROOT = repoRoot(import.meta.url);
const DEFAULT_GUIDE = join(REPO_ROOT, "docs", "guide");
// Under @ref[site/], the guide keeps its repository path so mdBook's edit link ({path}) maps onto it.
const DEFAULT_OUT = join(REPO_ROOT, "site", "docs", "guide");

const NAME = "[A-Za-z_][A-Za-z0-9_]*";
const TARGET = "[^\\]\\s]+";
// `(?<!\\)` leaves an escaped marker (`\@ref[x]`) alone; markdown renders it as literal text.
const REF = new RegExp(`(?<!\\\\)@ref\\[(${TARGET})(?: as (${NAME}))?\\]`, "g");
const USE = new RegExp(`(?<!\\\\)@\\[(${NAME})\\]`, "g");
const ANCHOR = new RegExp(`(?<!\\\\)@anchor\\[(${TARGET})\\]`, "g");
const NOREF = /(?<!\\)@noref\[[^\]]*\]/g;
const ANCHOR_COMMENT = new RegExp(`^\\s*<!--\\s*@anchor\\[(${TARGET})\\]\\s*-->\\s*$`);
const NOREF_COMMENT = /^\s*<!--\s*@noref\[[^\]]*\]\s*-->\s*$/;
const REFS_COMMENT = /^\s*<!--\s*refs\s*-->\s*$/;
const DECLARATION = `(?:@ref\\[${TARGET} as ${NAME}\\]|@noref\\[[^\\]]*\\])`;
const DECLARATION_LINE = new RegExp(`^\\s*${DECLARATION}(?:\\s+${DECLARATION})*\\s*$`);
const FENCE = /^\s{0,3}(`{3,}|~{3,})/;
const CODE_SPAN = /(`+)([^`]|[^`][\s\S]*?[^`])\1(?!`)/g;

function main() {
  const { values: args } = parseArgs({
    options: {
      guide: { type: "string", default: DEFAULT_GUIDE },
      out: { type: "string", default: DEFAULT_OUT },
      "mdbook-version": { type: "boolean", default: false },
    },
  });
  if (args["mdbook-version"]) {
    console.log(MDBOOK_VERSION);
    return;
  }
  const pages = loadPages(args.guide);
  const site = buildSite(pages);
  rmSync(args.out, { recursive: true, force: true });
  mkdirSync(args.out, { recursive: true });
  for (const [name, text] of site) {
    writeFileSync(join(args.out, name), text);
  }
  console.log(`wrote ${site.size} files to ${args.out}`);
}

// Only the CLI invocation runs; importing the module for tests must not read the repo or write.
if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main();
}

/// Reads the guide directory into `{ name, title, text }` records in `PAGES` order, refusing a
/// directory whose files and `PAGES` disagree.
export function loadPages(guideDir) {
  const docs = new Map(collectDocs(guideDir).map((doc) => [doc.path, doc]));
  const missing = PAGES.filter((name) => !docs.has(name));
  const unlisted = [...docs.keys()].filter((name) => !PAGES.includes(name));
  if (missing.length > 0 || unlisted.length > 0) {
    throw new Error(
      `${guideDir} and PAGES in scripts/src/site/build.mjs disagree: ` +
        `missing ${JSON.stringify(missing)}, unlisted ${JSON.stringify(unlisted)}`,
    );
  }
  return PAGES.map((name) => ({
    name,
    title: docs.get(name).title,
    text: readFileSync(join(guideDir, name), "utf8"),
  }));
}

/// The site source as a map of file name to contents: `SUMMARY.md` plus one file per page.
export function buildSite(pages) {
  const anchors = collectAnchors(pages);
  const files = new Map([["SUMMARY.md", renderSummary(pages)]]);
  for (const page of pages) {
    files.set(page.name, rewritePage(stripFrontmatter(page.text), { page, pages, anchors }));
  }
  return files;
}

export function renderSummary(pages) {
  const lines = ["# Summary", ""];
  for (const page of pages) {
    lines.push(`- [${page.title}](${page.name})`);
  }
  lines.push("");
  return lines.join("\n");
}

/// The text after the closing `---` of the frontmatter block, or the whole text when there is
/// no block. Validity is `gen-index`'s job; this only has to find the end.
export function stripFrontmatter(text) {
  const lines = text.split(/\r?\n/);
  if (lines[0] !== "---") return text;
  const close = lines.indexOf("---", 1);
  if (close === -1) return text;
  return lines.slice(close + 1).join("\n");
}

/// Every `@anchor[id]` declared in the guide, comment form or inline, outside code, mapped to
/// the page that declares it. A duplicate is an error here as it is for `anchr check`.
export function collectAnchors(pages) {
  const anchors = new Map();
  for (const page of pages) {
    for (const line of proseLines(stripFrontmatter(page.text))) {
      const ids = [];
      const comment = line.match(ANCHOR_COMMENT);
      if (comment) {
        ids.push(comment[1]);
      } else {
        for (const segment of proseSegments(line)) {
          for (const match of segment.matchAll(ANCHOR)) ids.push(match[1]);
        }
      }
      for (const id of ids) {
        if (anchors.has(id)) {
          throw new Error(`${page.name}: anchor \`${id}\` is already declared in ${anchors.get(id)}`);
        }
        anchors.set(id, page.name);
      }
    }
  }
  return anchors;
}

/// Rewrites one page's markers into links. `ctx` is `{ page, pages, anchors }`.
export function rewritePage(text, ctx) {
  const aliases = new Map();
  const out = [];
  let fence = null;
  for (const line of text.split(/\r?\n/)) {
    const fenceMatch = line.match(FENCE);
    if (fence === null && fenceMatch) {
      fence = fenceMatch[1];
      out.push(line);
      continue;
    }
    if (fence !== null) {
      if (fenceMatch && fenceMatch[1][0] === fence[0] && fenceMatch[1].length >= fence.length) {
        fence = null;
      }
      out.push(line);
      continue;
    }

    if (REFS_COMMENT.test(line) || NOREF_COMMENT.test(line)) continue;
    const anchorComment = line.match(ANCHOR_COMMENT);
    if (anchorComment) {
      out.push(anchorTag(anchorComment[1]));
      continue;
    }
    if (DECLARATION_LINE.test(line)) {
      for (const match of line.matchAll(REF)) declare(aliases, match[1], match[2], ctx);
      continue;
    }
    out.push(rewriteInline(line, aliases, ctx));
  }
  return collapseBlankLines(out).join("\n");
}

function rewriteInline(line, aliases, ctx) {
  return mapProse(line, (segment) =>
    segment
      .replace(ANCHOR, (_, id) => anchorTag(id))
      .replace(NOREF, "")
      .replace(REF, (_, target, alias) => {
        if (alias) declare(aliases, target, alias, ctx);
        return link(alias ?? codeText(target), target, ctx);
      })
      .replace(USE, (_, alias) => {
        if (!aliases.has(alias)) {
          throw new Error(`${ctx.page.name}: \`@[${alias}]\` is used before any declaration`);
        }
        return link(alias, aliases.get(alias), ctx);
      }),
  );
}

function declare(aliases, target, alias, ctx) {
  if (aliases.has(alias)) {
    throw new Error(`${ctx.page.name}: alias \`${alias}\` is declared twice`);
  }
  aliases.set(alias, target);
}

/// A markdown link for `target`, shown as `text` unless the target is a guide page, whose
/// title reads better than any alias.
function link(text, target, ctx) {
  const resolved = resolveTarget(target, ctx);
  return `[${resolved.title ?? text}](${resolved.href})`;
}

function resolveTarget(target, ctx) {
  const { page, pages, anchors } = ctx;
  if (/^[A-Za-z0-9_-]+:/.test(target)) {
    throw new Error(`${page.name}: \`@ref[${target}]\` points into another root; the site cannot link it`);
  }
  if (target.startsWith("#")) {
    const id = target.slice(1);
    const owner = anchors.get(id);
    if (!owner) {
      throw new Error(`${page.name}: \`@ref[${target}]\` names an anchor not declared in the guide`);
    }
    return { href: owner === page.name ? `#${id}` : `${owner}#${id}` };
  }
  const hash = target.indexOf("#");
  const rawPath = hash === -1 ? target : target.slice(0, hash);
  const isDirectory = rawPath.endsWith("/");
  const path = rootRelative(rawPath.replace(/\/$/, ""), page.name);
  const guidePrefix = `${GUIDE_DIR}/`;
  if (hash === -1 && path.startsWith(guidePrefix)) {
    const name = path.slice(guidePrefix.length);
    const other = pages.find((candidate) => candidate.name === name);
    if (other) return { href: name, title: other.title };
  }
  return { href: `${REPOSITORY_URL}/${isDirectory ? "tree" : "blob"}/main/${path}` };
}

/// A bare path is root-relative; `./` and `../` are relative to the page, which lives in the
/// guide directory.
function rootRelative(path, pageName) {
  if (path.startsWith("./") || path.startsWith("../")) {
    return posix.normalize(posix.join(GUIDE_DIR, posix.dirname(pageName), path));
  }
  return path;
}

function anchorTag(id) {
  return `<a id="${id}"></a>`;
}

function codeText(target) {
  return `\`${target}\``;
}

/// Applies `transform` to the parts of a line outside inline code.
function mapProse(line, transform) {
  let result = "";
  let last = 0;
  for (const match of line.matchAll(CODE_SPAN)) {
    result += transform(line.slice(last, match.index)) + match[0];
    last = match.index + match[0].length;
  }
  return result + transform(line.slice(last));
}

function* proseSegments(line) {
  const segments = [];
  mapProse(line, (segment) => {
    segments.push(segment);
    return segment;
  });
  yield* segments;
}

/// The lines of `text` that are outside code fences.
function* proseLines(text) {
  let fence = null;
  for (const line of text.split(/\r?\n/)) {
    const fenceMatch = line.match(FENCE);
    if (fence === null && fenceMatch) {
      fence = fenceMatch[1];
    } else if (fence !== null) {
      if (fenceMatch && fenceMatch[1][0] === fence[0] && fenceMatch[1].length >= fence.length) {
        fence = null;
      }
    } else {
      yield line;
    }
  }
}

/// Dropping a refs block leaves a run of blank lines behind the title; one is enough.
function collapseBlankLines(lines) {
  const out = [];
  for (const line of lines) {
    if (line.trim() === "" && out.length > 0 && out[out.length - 1].trim() === "") continue;
    out.push(line);
  }
  return out;
}
