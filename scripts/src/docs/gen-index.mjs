#!/usr/bin/env node
// Generates the developer-docs index from each document's frontmatter, so the index can never
// disagree with the files it lists:
// @noref[scripts/src/docs/gen-index.mjs, docs/README.md]
//
//   docs/README.md   one table per directory: title, description, tags
//
// Every `docs/**/*.md` except the index itself must start with a frontmatter block holding
// `title` and `description` (and optionally `tags`); a document without one is an error rather
// than a silently missing row. CI runs `--check` and fails on any diff.
//
//   node scripts/src/docs/gen-index.mjs [--docs <dir>] [--out <path>] [--check]

import { readdirSync, readFileSync, writeFileSync } from "node:fs";
import { join, relative, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";

import { repoRoot } from "../repo-root.mjs";

const INDEX_NAME = "README.md";
const REQUIRED_KEYS = ["title", "description"];

// A directory's heading in the index. Directories not listed here are headed by their name.
const GROUP_HEADINGS = {
  "": "Core",
  design: "Feature designs",
  guide: "User guide",
};

const REPO_ROOT = repoRoot(import.meta.url);
const DEFAULT_DOCS = join(REPO_ROOT, "docs");

function main() {
  const { values: args } = parseArgs({
    options: {
      docs: { type: "string", default: DEFAULT_DOCS },
      out: { type: "string" },
      check: { type: "boolean", default: false },
    },
  });
  const out = args.out ?? join(args.docs, INDEX_NAME);
  const rendered = render(collectDocs(args.docs));

  if (args.check) {
    const current = readFileSync(out, "utf8");
    if (current !== rendered) {
      console.error(`${out} is stale; rerun without --check to regenerate`);
      process.exit(1);
    }
    console.log(`${out} is up to date`);
  } else {
    writeFileSync(out, rendered);
    console.log(`wrote ${out}`);
  }
}

// Only the CLI invocation runs; importing the module for tests must not read the repo or write.
if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main();
}

export { collectDocs, groupDocs, parseFrontmatter, render };

/// Reads every markdown file under `docsDir` (except the index) and returns its parsed
/// frontmatter with a `path` relative to `docsDir`, using forward slashes.
function collectDocs(docsDir) {
  const docs = [];
  for (const file of markdownFiles(docsDir)) {
    const path = relative(docsDir, file).split(sep).join("/");
    if (path === INDEX_NAME) continue;
    docs.push({ path, ...parseFrontmatter(readFileSync(file, "utf8"), path) });
  }
  return docs;
}

function* markdownFiles(directory) {
  const entries = readdirSync(directory, { withFileTypes: true }).sort((a, b) =>
    a.name < b.name ? -1 : a.name > b.name ? 1 : 0,
  );
  for (const entry of entries) {
    const full = join(directory, entry.name);
    if (entry.isDirectory()) {
      yield* markdownFiles(full);
    } else if (entry.isFile() && entry.name.endsWith(".md")) {
      yield full;
    }
  }
}

// The block is `---`, `key: value` lines, `---`. Only the keys the index renders are read, and
// only as strings; `tags` is a bracketed, comma-separated list. Anything else is an error that
// names the file, because a missing row in the index is exactly the failure this script exists
// to prevent.
function parseFrontmatter(text, path) {
  const lines = text.split(/\r?\n/);
  if (lines[0] !== "---") {
    throw new Error(`${path}: missing frontmatter (the first line must be ---)`);
  }
  const fields = {};
  let closed = false;
  for (const line of lines.slice(1)) {
    if (line === "---") {
      closed = true;
      break;
    }
    if (line.trim() === "") continue;
    const match = line.match(/^([a-z_]+):\s*(.*?)\s*$/);
    if (!match) {
      throw new Error(`${path}: frontmatter line is not \`key: value\`: ${line}`);
    }
    fields[match[1]] = match[2];
  }
  if (!closed) {
    throw new Error(`${path}: frontmatter is not closed with ---`);
  }
  for (const key of REQUIRED_KEYS) {
    if (!fields[key]) {
      throw new Error(`${path}: frontmatter is missing \`${key}\``);
    }
  }
  return {
    title: fields.title,
    description: fields.description,
    tags: parseTags(fields.tags ?? "", path),
  };
}

function parseTags(raw, path) {
  if (raw === "") return [];
  const match = raw.match(/^\[(.*)\]$/);
  if (!match) {
    throw new Error(`${path}: tags must be a bracketed list: ${raw}`);
  }
  return match[1]
    .split(",")
    .map((tag) => tag.trim())
    .filter((tag) => tag !== "");
}

/// Groups docs by directory: the top level first, then subdirectories by name, files by path.
function groupDocs(docs) {
  const groups = new Map();
  for (const doc of docs) {
    const directory = doc.path.includes("/") ? doc.path.slice(0, doc.path.lastIndexOf("/")) : "";
    if (!groups.has(directory)) groups.set(directory, []);
    groups.get(directory).push(doc);
  }
  const byName = (a, b) => (a < b ? -1 : a > b ? 1 : 0);
  return [...groups.keys()]
    .sort((a, b) => (a === "" ? -1 : b === "" ? 1 : byName(a, b)))
    .map((directory) => ({
      directory,
      heading: GROUP_HEADINGS[directory] ?? directory,
      docs: groups.get(directory).sort((a, b) => byName(a.path, b.path)),
    }));
}

function render(docs) {
  const lines = [
    "<!-- Generated by @ref[scripts/src/docs/gen-index.mjs] from each document's frontmatter;",
    "     do not edit by hand. Edit the frontmatter and rerun the script. -->",
    "",
    "# Documentation",
    "",
    "How anchr is built, tested, and shipped, and the user guide. Each document says at the top",
    "when to read it. The user guide is published at https://arkorri.github.io/context-anchors/;",
    "the marker syntax written into a project by `anchr init` is the same grammar in one page.",
  ];
  for (const group of groupDocs(docs)) {
    lines.push("", `## ${group.heading}`, "", "| Doc | Description | Tags |", "|---|---|---|");
    for (const doc of group.docs) {
      lines.push(`| [${doc.title}](${doc.path}) | ${escapeCell(doc.description)} | ${doc.tags.join(", ")} |`);
    }
  }
  lines.push("");
  return lines.join("\n");
}

function escapeCell(text) {
  return text.replaceAll("|", "\\|");
}
