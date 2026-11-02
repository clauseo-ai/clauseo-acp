import { Lexer } from "marked";
import { expect, test, vi } from "vitest";
import spec from "commonmark-spec/spec.txt?raw";
import { createAnswer } from "./answer.ts";
import gfmSpec from "./gfm-spec.json";
import { CASES } from "./hostile-cases.ts";
import { capNesting } from "./nesting.ts";
import { endOf, parse } from "./parse.ts";
import { examplesOf, htmlOf, normalise, unsplit, withTabs } from "./spec-fixture.ts";
import { createSplitter, MAX_PARAGRAPH } from "./split.ts";
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
  // The syntax extras and the inline tags (#137 §3 and §4).
  "**Term**",
  ": a definition",
  "   : an indented definition",
  "Section 3^A^, the 2^nd^ notice, 2^10 and [^n]",
  "==highlighted== and a == b",
  "a <sup>tag that opens",
  "and closes</sup> <br> <kbd>Ctrl</kbd>",
  "<br>",
  "## Notes",
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

/** Paragraphs longer than MAX_PARAGRAPH, with block markers mid-line that must never start a part. */
const LONG_PARAGRAPHS = [
  "Lorem ipsum dolor sit amet, **consectetur** adipiscing [elit](u), sed do _eiusmod_ tempor.\n".repeat(
    150,
  ),
  "word - item 1. two # three > four ``` five | six 7) seven <pre> ".repeat(200),
  capNesting("a==b==".repeat(3000)),
  capNesting("^a^".repeat(6000)),
  "<u>a</u>".repeat(2000),
  `Intro.\n\n${"x".repeat(9000)}\n\nOutro.\n`,
];

test("a paragraph longer than MAX_PARAGRAPH is cut into parts that start with text; the cuts never move", () => {
  for (const text of LONG_PARAGRAPHS) {
    const incremental = createSplitter(realEndOf);
    for (let end = 16; end < text.length + 16; end += 16) {
      const prefix = text.slice(0, Math.min(end, text.length));
      const blocks = incremental(prefix);
      expect(blocks.join("")).toBe(prefix);
      expect(blocks).toEqual(fresh(prefix));
    }
    const blocks = fresh(text);
    expect(blocks.length, text.slice(0, 40)).toBeGreaterThan(1);
    for (const [index, block] of blocks.entries()) {
      expect(block.length).toBeLessThanOrEqual(MAX_PARAGRAPH);
      // A part after a cut starts with a letter, a tag or a stand-in, so it is a paragraph too.
      if (index > 0 && !blocks[index - 1]!.endsWith("\n\n"))
        expect(block).toMatch(/^(?:\p{L}|<\/?[A-Za-z]|[\uFDD1-\uFDE0])/u);
    }
  }
  // Tags stay whole: every part is whole pairs.
  for (const block of fresh("<u>a</u>".repeat(2000))) expect(block).toMatch(/^(?:<u>a<\/u>)+$/);
}, 30_000);

test("a footnote's definition is never cut, however long its note", () => {
  const note = `[^n]: ${"a ".repeat(4000)}\n`;
  expect(fresh(note)).toEqual([note]);
});

test("a change before the text's end splits as a fresh split does, from the changed block on", () => {
  let updates = 0;
  for (const text of [...SPEC_TEXTS.slice(0, 200), ...GENERATED.slice(0, 50)]) {
    const incremental = createSplitter(realEndOf);
    for (let end = 2; end <= text.length; end += 3) {
      // The last character turns into another, as capNesting's stand-ins do, then the text grows on.
      const prefix = text.slice(0, end);
      for (const changed of [`${prefix.slice(0, -1)}\uFDDD`, prefix]) {
        expect(incremental(changed), JSON.stringify(changed)).toEqual(fresh(changed));
        updates++;
      }
    }
  }
  expect(updates).toBeGreaterThan(10_000);
}, 60_000);

test("an update lexes from the block it changes, not from the start: capNesting's tail changes cost no more", () => {
  // 100 KB of prose, then `"a==b==".repeat(3000)`: past nesting.ts's 400 sequences, a `=` that ends one
  // update is text, and a stand-in once the next `=` arrives. That change, before the end of the text,
  // made the splitter lex the whole answer again (12.6 s in JavaScriptCore for the 540 KB hostile corpus).
  const markdown = `${"Prose of an ordinary answer, one paragraph after another.\n\n".repeat(1700)}${"a==b==".repeat(3000)}`;
  const lexed: number[] = [];
  const spy = vi.spyOn(Lexer.prototype, "blockTokens");
  const split = createSplitter(realEndOf);
  let tailChanges = 0;
  let previous = "";
  try {
    for (let end = 100_000; end <= markdown.length; end += 16) {
      const text = capNesting(markdown.slice(0, end));
      if (!text.startsWith(previous)) tailChanges++;
      previous = text;
      spy.mockClear();
      split(text);
      // The first update splits the 100 KB it starts with.
      if (end > 100_000)
        lexed.push(Math.max(0, ...spy.mock.calls.map(([source]) => source.length)));
    }
  } finally {
    spy.mockRestore();
  }
  expect(tailChanges).toBeGreaterThan(100);
  // The 18 KB paragraph is lexed whole from its start (its parts' cuts are decided from there).
  expect(Math.max(...lexed)).toBeLessThan(20_000);
}, 60_000);

/**
 * A document as the one-piece comparison takes it, with one micromark 4.0.3 quirk: an ordered item
 * starting at a number other than 1, right after an indented line and a blank line, is a paragraph where
 * CommonMark (and commonmark.js 0.31.2) starts a list; split there, we give CommonMark's answer (see the
 * test below), so one piece is the wrong reference, and only such an item starts at 1 here.
 */
const AFTER_INDENTED = /^((?: {4}|\t)[^\n]*\S[^\n]*\n(?:[ \t]*\n)+)\d{1,9}([.)])(?=[ \t\n])/gm;
const comparable = (text: string) =>
  text.replace(AFTER_INDENTED, (_, before: string, delimiter: string) => `${before}1${delimiter}`);

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

// JavaScriptCore (WebKitGTK, and WKWebView on a Mac) cannot compile marked's setext-heading regex, whose
// lookaheads hold repeated groups, so it runs in its regex interpreter, one group iteration per character
// of the paragraph, over every paragraph of every update: 18 to 60 ms for 15 KB where V8 takes 0.1 ms, and
// `"a~ ".repeat(5000)` took 98 s to stream in WebKitGTK. A wall-clock limit in Node cannot see that; this
// test checks that the splitter never runs the regex at all.
test("the splitter never runs marked's setext-heading regex, and a setext heading is still a block", () => {
  const lheading = Lexer.rules.block.gfm.lheading;
  const exec = vi.spyOn(lheading, "exec");
  try {
    expect(fresh("Title\n===\n\nSub\n---\n\nText\n")).toEqual([
      "Title\n===\n\n",
      "Sub\n---\n\n",
      "Text\n",
    ]);
    for (const c of CASES) fresh(capNesting(c.markdown));
    expect(exec).not.toHaveBeenCalled();
  } finally {
    exec.mockRestore();
  }
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

test("a block that is one ATX heading line is cut after it without a blank line; nothing else is", () => {
  expect(fresh("# a\n## b\n# c")).toEqual(["# a\n", "## b\n", "# c"]);
  expect(fresh("# a\ntext\n# b\n")).toEqual(["# a\n", "text\n# b\n"]);
  expect(fresh("# a\n    code\n")).toEqual(["# a\n    code\n"]);
  expect(fresh("> q\n# a\nb\n")).toEqual(["> q\n# a\nb\n"]);
  for (const text of ["# a\n# b\n- c\n- d\n", "# a\n```\nx\n```\n# b\n", "# a\n<div>\n# b\n"])
    expect(normalise(htmlOf(createAnswer("spec").update(text, false)))).toBe(
      normalise(unsplit(text)),
    );
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

test("a finished block's view holds its own source, not the streaming text that parsed the same", () => {
  const answer = createAnswer("t");
  const [streamed] = answer.update("The **key", true);
  const [finished] = answer.update("The **key**", false);
  expect(streamed?.source).toBe("The **key");
  expect(finished?.input).toBe(streamed?.input);
  expect(finished?.source).toBe("The **key**");
});
