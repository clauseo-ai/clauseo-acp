import { expect, test } from "vitest";
import spec from "commonmark-spec/spec.txt?raw";
import { createAnswer } from "./answer.ts";
import gfmSpec from "./gfm-spec.json";
import { CASES } from "./hostile-cases.ts";
import { endOf, parse } from "./parse.ts";
import { examplesOf, htmlOf, normalise, unsplit, withTabs } from "./spec-fixture.ts";
import { createSplitter } from "./split.ts";
import type { BlockEnd } from "./split.ts";

const ends = new Map<string, BlockEnd>();
const realEndOf = (block: string) => {
  let end = ends.get(block);
  if (!end) ends.set(block, (end = endOf(block, parse(block))));
  return end;
};
const fresh = (text: string) => createSplitter(realEndOf)(text);

/** A seeded generator (mulberry32), so every run sees the same documents. */
function random(seed: number) {
  let state = seed;
  return () => {
    state = (state + 0x6d2b79f5) | 0;
    let t = Math.imul(state ^ (state >>> 15), 1 | state);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

/** Lines that start, continue or end every kind of block, including the ones that run past blank lines. */
const PIECES = [
  "A paragraph with *emphasis* and a [ref].",
  "lazy continuation",
  "# Heading",
  "Setext",
  "===",
  "---",
  "- item",
  "* item",
  "+ item",
  "1. item",
  "2) item",
  "10. item",
  "  continued in the item",
  "   - nested",
  "    indented code",
  "\tindented by a tab",
  "> quoted",
  "> > nested quote",
  ">",
  "```js",
  "```",
  "````",
  "~~~",
  "~~~~ py",
  "<!-- comment",
  "-->",
  "<script>",
  "</script>",
  "<pre>",
  "</pre>",
  "<div>",
  "</div>",
  "<?php",
  "?>",
  "<!DOCTYPE html",
  ">",
  "<![CDATA[",
  "]]>",
  "[ref]: /url 'Title'",
  "[Ref]: /other",
  "> [quoted]: /in-quote",
  "- [listed]: /in-list",
  "[quoted] and [listed] and [ref][]",
  "[^n]: a note",
  "    more of the note",
  "a note call[^n]",
  "| a | b |",
  "|---|:-:|",
  "| 1 | 2 |",
  "* * *",
  "- [ ] task",
  "Text with `code",
  "end` of code",
  "**bold",
  "www.example.com",
  "<https://example.com>",
  "\\",
  "",
];

function document(next: () => number): string {
  const lines: string[] = [];
  const count = 4 + Math.floor(next() * 24);
  for (let index = 0; index < count; index++) {
    lines.push(PIECES[Math.floor(next() * PIECES.length)] ?? "");
    const gap = next();
    if (gap < 0.5) lines.push("");
    if (gap < 0.05) lines.push("");
  }
  return `${lines.join("\n")}${next() < 0.5 ? "\n" : ""}`;
}

const GENERATED = Array.from({ length: 500 }, (_, index) => document(random(index + 1)));
const SPEC_TEXTS = [
  ...examplesOf(spec).map((example) => example.markdown),
  ...gfmSpec.examples.map((example) => withTabs(example).markdown),
];

test("for every prefix the incremental split equals a fresh split, and the blocks concatenate to the input", () => {
  let prefixes = 0;
  for (const text of [
    ...SPEC_TEXTS,
    ...GENERATED,
    ...CASES.map((c) => c.markdown).filter((m) => m.length < 2000),
  ]) {
    const incremental = createSplitter(realEndOf);
    for (let end = 1; end <= text.length; end++) {
      const prefix = text.slice(0, end);
      const blocks = incremental(prefix);
      expect(blocks.join(""), JSON.stringify(prefix)).toBe(prefix);
      expect(blocks, JSON.stringify(prefix)).toEqual(fresh(prefix));
      prefixes++;
    }
  }
  expect(prefixes).toBeGreaterThan(100_000);
}, 30_000);

/**
 * A document as the one-piece comparison takes it. Footnotes are left out (`[^` becomes `[`, a link
 * definition and reference): a note in one block and its call in another do not meet yet, which is the
 * document-wide footnote work that follows this renderer. And one micromark 4.0.3 quirk: an ordered item
 * starting at a number other than 1, right after an indented line and a blank line, is a paragraph where
 * CommonMark (and commonmark.js 0.31.2) starts a list; split there, we give CommonMark's answer (see the
 * test below), so one piece is the wrong reference, and only such an item starts at 1 here.
 */
const AFTER_INDENTED = /^((?: {4}|\t)[^\n]*\S[^\n]*\n(?:[ \t]*\n)+)\d{1,9}([.)])(?=[ \t\n])/gm;
const comparable = (text: string) =>
  text
    .replaceAll("[^", "[")
    .replace(AFTER_INDENTED, (_, before: string, delimiter: string) => `${before}1${delimiter}`);

test("rendered block by block, every document renders exactly as it does in one piece", () => {
  let cuts = 0;
  for (const text of [...SPEC_TEXTS, ...GENERATED].map(comparable)) {
    const views = createAnswer("spec").update(text, false);
    cuts += views.length - 1;
    expect(normalise(htmlOf(views)), JSON.stringify(text)).toBe(normalise(unsplit(text)));
  }
  expect(cuts).toBeGreaterThan(1000);
}, 30_000);

test("prose, headings, lists and closed fences split into blocks at blank lines", () => {
  const text =
    "# Advice\n\nFirst para.\n\n- a\n- b\n\n```ts\nconst x = 1;\n\nconst y = 2;\n```\n\nLast para.\n";
  expect(fresh(text)).toEqual([
    "# Advice\n\n",
    "First para.\n\n",
    "- a\n- b\n\n",
    "```ts\nconst x = 1;\n\nconst y = 2;\n```\n\n",
    "Last para.\n",
  ]);
});

test.each([
  ["a list continuing over a blank line", "- a\n\n- b\n"],
  ["an ordered list continuing over a blank line", "1. a\n\n2. b\n"],
  ["an item's indented continuation", "- a\n\n  more of a\n"],
  ["a lazy continuation line", "> quote\nlazy\n"],
  ["a setext underline", "Title\n===\n"],
  ["an indented code block over a blank line", "    one\n\n    two\n"],
  ["an open fence with blank lines", "```\ncode\n\nmore\n"],
  ["a fence marked closed by marked but not by CommonMark", "```\ncode\n```~~\n\nstill code\n"],
  ["an HTML comment over blank lines", "<!-- a\n\nb -->\n"],
  ["a script block over blank lines", "<script>\n\nx\n</script>\n"],
  ["a processing instruction over blank lines", "<?php\n\nx ?>\n"],
  ["a declaration over blank lines", "<!DOCTYPE\n\nhtml>\n"],
  ["a CDATA section over blank lines", "<![CDATA[\n\nx]]>\n"],
  ["a footnote definition with a second paragraph", "[^1]: note\n\n    more of the note\n"],
])("one block: %s", (_, text) => {
  expect(fresh(text)).toEqual([text]);
});

test("an ordered list after an indented code block starts at its own number, as in CommonMark", () => {
  expect(htmlOf(createAnswer("t").update("    code\n\n10. item\n", false))).toContain(
    '<ol start="10">',
  );
});

test("a list followed by a list of another kind is cut, since the first list has ended", () => {
  expect(fresh("- a\n\n1. b\n")).toEqual(["- a\n\n", "1. b\n"]);
  expect(fresh("- a\n\n* b\n")).toEqual(["- a\n\n", "* b\n"]);
});

test("a reference resolves against a definition in a later block, in a block quote or in a list item", () => {
  const html = htmlOf(
    createAnswer("t").update(
      "See [one], [two] and [three].\n\nMore.\n\n> [two]: /two\n\n- [three]: /three\n\n[one]: /one\n",
      false,
    ),
  );
  expect(html).toContain('<a href="/one">one</a>');
  expect(html).toContain('<a href="/two">two</a>');
  expect(html).toContain('<a href="/three">three</a>');
});

test("the first definition of a label wins, across blocks, and case and spacing do not matter", () => {
  const html = htmlOf(
    createAnswer("t").update("[Foo  Bar]\n\n[foo bar]: /first\n\nx\n\n[FOO BAR]: /second\n", false),
  );
  expect(html).toContain('<a href="/first">Foo  Bar</a>');
});

test("definitions round-trip: escapes, entities, angle brackets, line endings in titles", () => {
  const text = '[a]\n\n[a]: <my url\\>&amp;> "say \\"hi\\"\nthere &amp; \\\\"\n';
  expect(normalise(htmlOf(createAnswer("t").update(text, false)))).toBe(normalise(unsplit(text)));
});

test("while streaming, the last block's definitions wait; at the end they count", () => {
  const answer = createAnswer("t");
  const text = "[x] here.\n\nMore.\n\n[x]: /x\n";
  expect(htmlOf(answer.update(text, true))).not.toContain("<a");
  expect(htmlOf(answer.update(text, false))).toContain('<a href="/x">x</a>');
});

test("a finished block keeps its view while the tail streams", () => {
  const answer = createAnswer("t");
  const first = answer.update("Para one.\n\nPara two.\n\nTail", true);
  const second = answer.update("Para one.\n\nPara two.\n\nTail grows", true);
  expect(second[0]).toBe(first[0]);
  expect(second[1]).toBe(first[1]);
  expect(second[2]).not.toBe(first[2]);
});

test("only the streaming last block gets tail repair", () => {
  const answer = createAnswer("t");
  expect(htmlOf(answer.update("**bold\n\n**open", true))).toBe(
    "<p>**bold</p>\n<p><strong>open</strong></p>",
  );
  expect(htmlOf(answer.update("**bold\n\n**open", false))).toBe("<p>**bold</p>\n<p>**open</p>");
});

test("CRLF and CR line endings split like LF", () => {
  expect(htmlOf(createAnswer("t").update("a\r\n\r\nb\rc", false))).toBe(
    htmlOf(createAnswer("t").update("a\n\nb\nc", false)),
  );
});
