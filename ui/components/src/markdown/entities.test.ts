import { execFileSync } from "node:child_process";
import { readdirSync, readFileSync } from "node:fs";
import { expect, test } from "vitest";
import { createAnswer } from "./answer.ts";
import { characterEntitiesLoaded, loadEntities } from "./entities.ts";

// micromark decodes named character references (`&amp;`, `&copy;`) with decode-named-character-reference.
// ui/patches/ replaces it (ui/pnpm-workspace.yaml, patchedDependencies): its browser build decoded through
// innerHTML, which the app's Trusted Types CSP refuses, and its other build holds the whole HTML table
// (character-entities, about 10 KB brotli). The patched module knows the common names at once and imports
// the table lazily. Vitest resolves with Node's conditions, so the graph is walked here the way a browser
// bundle resolves it: in a child Node with the browser condition, from the renderer's own imports.

/** The renderer's product files: what a consumer's first load takes in from this folder. */
const PRODUCT = readdirSync(new URL(".", import.meta.url)).filter(
  (file) => /\.tsx?$/.test(file) && !/\.test\.tsx?$|-fixture\.ts$|^hostile-cases\.ts$/.test(file),
);

/** The bare packages the renderer imports (not types, not React, which the app already loads). */
function rendererPackages(): string[] {
  const found = new Set<string>();
  for (const file of PRODUCT) {
    const text = readFileSync(new URL(file, import.meta.url), "utf8");
    for (const [, kind, specifier] of text.matchAll(
      /^(?:import|export)(\s+type)?[^;]*?from\s+"([^"./][^"]*)";/gms,
    )) {
      if (!kind && !/^react(?:-dom)?(?:\/|$)/.test(specifier!)) found.add(specifier!);
    }
  }
  return [...found];
}

/** In a child Node (browser condition): every file reachable by static imports, and the lazy imports seen. */
const WALK = `
import { readFileSync } from "node:fs";
import { fileURLToPath, pathToFileURL } from "node:url";
const [parent, ...roots] = JSON.parse(process.argv[1]);
const files = new Set(), lazy = new Set();
const visit = (specifier, from) => {
  const url = import.meta.resolve(specifier, from);
  if (!url.startsWith("file:") || files.has(url)) return;
  files.add(url);
  const text = readFileSync(fileURLToPath(url), "utf8").replace(/\\/\\*[\\s\\S]*?\\*\\/|^\\s*\\/\\/.*$/gm, "");
  for (const m of text.matchAll(/^\\s*(?:import|export)\\s+(?:[^'"]*?\\sfrom\\s+)?['"]([^'"]+)['"]/gm)) visit(m[1], url);
  for (const m of text.matchAll(/\\bimport\\(\\s*['"]([^'"]+)['"]\\s*\\)/g)) lazy.add(import.meta.resolve(m[1], url));
};
for (const root of roots) visit(root, pathToFileURL(parent).href);
process.stdout.write(JSON.stringify({ files: [...files].map(fileURLToPath), lazy: [...lazy].map(fileURLToPath) }));
`;

function browserGraph(): { files: string[]; lazy: string[] } {
  const parent = new URL("entities.ts", import.meta.url).pathname;
  const output = execFileSync(
    process.execPath,
    [
      "--conditions=browser",
      "--experimental-import-meta-resolve",
      "--no-warnings",
      "--input-type=module",
      "--eval",
      WALK,
      JSON.stringify([parent, ...rendererPackages()]),
    ],
    { encoding: "utf8" },
  );
  return JSON.parse(output) as { files: string[]; lazy: string[] };
}

const GRAPH = browserGraph();
const CHARACTER_ENTITIES = /[\\/]character-entities[\\/]index\.js$/;

test("the walk sees the renderer's parser and the patched decoder", () => {
  expect(rendererPackages()).toContain("mdast-util-from-markdown");
  expect(GRAPH.files.length).toBeGreaterThan(100);
  expect(GRAPH.files).toContainEqual(
    expect.stringMatching(/decode-named-character-reference[\\/]index\.js$/),
  );
});

test("the first load holds no full entity table: it is a lazy import of its own", () => {
  expect(GRAPH.files.filter((file) => CHARACTER_ENTITIES.test(file))).toEqual([]);
  const tables = GRAPH.files.filter((file) =>
    readFileSync(file, "utf8").includes("CounterClockwiseContourIntegral"),
  );
  expect(tables).toEqual([]);
  expect(GRAPH.lazy).toContainEqual(expect.stringMatching(CHARACTER_ENTITIES));
});

test("the browser-resolved code sets no innerHTML and parses no HTML", () => {
  const sinks = GRAPH.files.filter((file) =>
    /\binnerHTML\b|\bouterHTML\b|insertAdjacentHTML|DOMParser|createContextualFragment|document\.write/.test(
      readFileSync(file, "utf8"),
    ),
  );
  expect(sinks).toEqual([]);
});

/** The text of an answer's first paragraph. */
function paragraphText(markdown: string): string {
  const [view] = createAnswer("t").update(markdown, false);
  const paragraph = view?.hast.children[0];
  return paragraph?.type === "element" && paragraph.children[0]?.type === "text"
    ? paragraph.children[0].value
    : "";
}

test("common references decode before the full table has loaded; other names stay as written", () => {
  expect(characterEntitiesLoaded()).toBe(false);
  expect(paragraphText("AT&amp;T, &copy; 2026 &sect;&nbsp;4 &mdash; &hellip; &bogus;")).toBe(
    "AT&T, © 2026 §\u00a04 — … &bogus;",
  );
  expect(paragraphText("&ngE; &frac12;")).toBe("&ngE; ½");
});

const TEXT = "Plain block.\n\nA rare &ngE; here.\n\nA common &amp; one.\n\nLast.\n";
const KNOWN = "Only &amp; and &copy; here.\n\nAnd text.\n";

test("once the table arrives an answer re-parses its blocks holding `&`; the others keep their views", async () => {
  expect(characterEntitiesLoaded()).toBe(false);
  const missing = createAnswer("a");
  const known = createAnswer("b");
  const before = missing.update(TEXT, true);
  const knownBefore = known.update(KNOWN, false);
  expect(JSON.stringify(before[1]!.hast)).toContain("&ngE;");

  await loadEntities();
  expect(characterEntitiesLoaded()).toBe(true);
  const after = missing.update(TEXT, true);
  expect(after[0]).toBe(before[0]);
  expect(after[3]).toBe(before[3]);
  expect(after[1]).not.toBe(before[1]);
  expect(JSON.stringify(after[1]!.hast)).toContain("≧̸");
  // An answer whose every reference was known keeps every view.
  const knownAfter = known.update(KNOWN, false);
  expect(knownAfter).toHaveLength(knownBefore.length);
  knownAfter.forEach((view, index) => expect(view).toBe(knownBefore[index]));
});

test("with the table loaded every HTML name decodes", () => {
  expect(characterEntitiesLoaded()).toBe(true);
  expect(paragraphText("&ngE; &frac12; &CounterClockwiseContourIntegral; &bogus;")).toBe(
    "≧̸ ½ ∳ &bogus;",
  );
});
