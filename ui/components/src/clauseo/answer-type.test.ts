// @vitest-environment happy-dom
// The answer's Markdown takes the reply type and the thread's own styles, not the browser's or the reset's: a
// numbered list keeps its numbers, paragraphs keep their rhythm, a heading stands apart, a wide table scrolls inside
// the answer. thread.css is read as written and applied over a reset like Tailwind's preflight, which takes every
// list's markers away; the test shows the reset alone does take them, so it can't pass by reading nothing.
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { afterEach, expect, test } from "vitest";

// happy-dom gives import.meta.url an http scheme, so the sheets are read by the file's own folder
const read = (path: string) => readFileSync(join(import.meta.dirname, path), "utf8");
const THREAD = read("thread.css");
const THEME = read("../styles/tokens-theme.css");
const TYPE = read("../styles/tokens-type.css");

/** Preflight's part that matters here: lists without markers or indent, block margins gone. */
const RESET =
  "ol, ul { list-style-type: none; margin: 0; padding: 0; } p, h1, h2, h3 { margin: 0; }";

/** thread.css as plain CSS: its layer unwrapped and Tailwind's @apply lines left out. */
function threadSheet(): string {
  const open = "@layer components {";
  const body = THREAD.slice(THREAD.indexOf(open) + open.length, THREAD.lastIndexOf("}"));
  return body.replace(/^\s*@apply [^;]+;$/gm, "");
}

/** The token values the answer's rules use, so var() resolves. */
const TOKENS =
  ":root { --s1: 4px; --s3: 12px; --s5: 20px; --s6: 24px; --t22: 22px; --t18: 18px; --w-chip: 520; }";

function setUp(sheets: string[]) {
  for (const css of sheets) {
    const style = document.createElement("style");
    style.textContent = css;
    document.head.append(style);
  }
  document.body.innerHTML = `<div class="reply-text"><div data-clauseo="markdown">
    <p>First.</p><p>Second.</p><h2>Grounds</h2><ol start="3"><li>Ground three</li></ol><ul><li>A point</li></ul>
    <table><tbody><tr><td>cell</td></tr></tbody></table></div></div>`;
}

const style = (selector: string) => {
  const node = document.querySelector(selector);
  if (node === null) throw new Error(selector);
  return getComputedStyle(node);
};

afterEach(() => {
  document.head.innerHTML = "";
  document.body.innerHTML = "";
});

test("the reset alone takes a numbered list's numbers away", () => {
  setUp([TOKENS, RESET]);
  expect(style("ol").listStyleType).toBe("none");
});

test("in an answer, a numbered list keeps its numbers and its indent, and a bulleted one its bullets", () => {
  setUp([TOKENS, RESET, threadSheet()]);
  expect(style("ol").listStyleType).toBe("decimal");
  expect(style("ol").paddingLeft).toBe("24px");
  expect(style("ul").listStyleType).toBe("disc");
});

test("paragraphs keep their rhythm, a heading stands apart at the section heads' weight, a table scrolls", () => {
  setUp([TOKENS, RESET, threadSheet()]);
  expect(style("p + p").marginTop).toBe("12px");
  expect(style("h2").marginTop).toBe("20px");
  expect(style("h2").fontWeight).toBe("520");
  expect(style("table").overflowX).toBe("auto");
});

test("a blockquote after a table or a list takes the block gap, --s3 above it, as the blocks before it do", () => {
  const PARTS = read("../styles/tokens-parts.css");
  const gap = PARTS.match(/--s3: (\d+px);/)?.[1];
  if (gap === undefined) throw new Error("--s3 is not a px value in tokens-parts.css");
  setUp([TOKENS, RESET, threadSheet()]);
  document.querySelector('[data-clauseo="markdown"]')!.innerHTML = `
    <table><tbody><tr><td>cell</td></tr></tbody></table>
    <blockquote><p>After the table.</p></blockquote>
    <ul><li>A point</li></ul>
    <blockquote><p>After the list.</p><p>Last.</p></blockquote>`;
  expect(style("table + blockquote").marginTop).toBe(gap);
  expect(style("ul + blockquote").marginTop).toBe(gap);
  // the quote's own first child takes no gap, and its last none at the foot: only children after a first are spaced
  expect(style("blockquote > p").marginTop).toBe("0px");
  expect(style("blockquote:last-child > p:last-child").marginBottom).toBe("0px");
  // happy-dom does not model :is() specificity, so the computed gaps above pass with or without the reset's
  // selector. In a browser the reset outranks the gap unless it is written with :where (Chromium: 0 px with :is,
  // 12 px with :where), so the selector is pinned as written
  expect(THREAD).toMatch(
    /\[data-clauseo="markdown"\]\s*:where\(p, ul, ol, dl, blockquote, pre, table, hr, h1, h2, h3, h4, h5, h6\)\s*\{\s*margin: 0;/,
  );
});

test("the answer is set in the reply type: 17 px at Clauseo's weight", () => {
  expect(THREAD).toMatch(/\.reply-text \{\s*@apply text-reply font-clauseo;/);
  const theme = THEME;
  const type = TYPE;
  expect(theme).toMatch(/--text-reply: var\(--t17\);/);
  expect(theme).toMatch(/--font-weight-clauseo: var\(--w-clauseo\);/);
  expect(type).toMatch(/--t17: 1\.0625rem;/);
  expect(type).toMatch(/--w-clauseo: 400;/);
});
