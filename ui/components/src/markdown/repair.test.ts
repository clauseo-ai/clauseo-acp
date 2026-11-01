// Tail repair (repair.ts) at the answer model: what the streaming last block shows for every half-arrived
// construct, that nothing shows its markers, that no link appears before it is complete, and that a
// repaired tail needs no second repair (every prefix of the spec examples and of the legal answer).
import { expect, test } from "vitest";
import spec from "commonmark-spec/spec.txt?raw";
import { createAnswer } from "./answer.ts";
import { LEGAL } from "./legal-fixture.ts";
import { endOf, parse } from "./parse.ts";
import { MAX_CLOSERS, repairTail } from "./repair.ts";
import { examplesOf, htmlOf } from "./spec-fixture.ts";
import { createSplitter } from "./split.ts";

/** The HTML of the streaming last block of `text`. */
function tail(text: string): string {
  const views = createAnswer("t").update(text, true);
  return htmlOf(views.slice(-1));
}

test.each([
  ["open bold", "The **key", "<p>The <strong>key</strong></p>"],
  ["open italic", "an *open", "<p>an <em>open</em></p>"],
  ["open underscore italic", "an _open", "<p>an <em>open</em></p>"],
  ["open underscore bold", "a __strong", "<p>a <strong>strong</strong></p>"],
  ["open strikethrough", "a ~~struck", "<p>a <del>struck</del></p>"],
  ["a strikethrough's closer half there", "a ~~struck~", "<p>a <del>struck</del></p>"],
  ["a bold closer half there", "a **bold*", "<p>a <strong>bold</strong></p>"],
  ["open code", "run `npm", "<p>run <code>npm</code></p>"],
  ["code holding an asterisk", "run `a*b", "<p>run <code>a*b</code></p>"],
  ["bold and italic inside", "**a *b", "<p><strong>a <em>b</em></strong></p>"],
  ["a closer after a space", "*foo bar *", "<p><em>foo bar</em></p>"],
  ["a marker with nothing after it yet", "The cap is **", "<p>The cap is</p>"],
  ["a strikethrough marker with nothing after it", "the draft's ~~", "<p>the draft's</p>"],
  ["Section 2(a)_ and", "Section 2(a)_ and", "<p>Section 2(a)_ and</p>"],
  ["an underscore inside a word", "the snake_case name", "<p>the snake_case name</p>"],
  ["an asterisk between spaces", "2 * 3 is", "<p>2 * 3 is</p>"],
  ["escaped literals", "Literal \\*\\*not bold\\*\\*", "<p>Literal **not bold**</p>"],
  ["an escape half there", "Literal \\*\\*not bold\\", "<p>Literal **not bold</p>"],
  ["see [the Act", "see [the Act", "<p>see the Act</p>"],
  ["a link's text, closed", "see [the Act]", "<p>see the Act</p>"],
  ["a link's destination half there", "see [the Act](https://indiacode.nic", "<p>see the Act</p>"],
  ["a full reference half there", "see [the Act][ac", "<p>see the Act</p>"],
  ["bold around a half link", "**see [the Act", "<p><strong>see the Act</strong></p>"],
  ["a half image", "the page ![Signed](clauseo-file:sig", "<p>the page</p>"],
  ["a half footnote reference", "the agreement.[^surv", "<p>the agreement.</p>"],
  ["a half-arrived tag", "if a<b then the clause", "<p>if a&#x3C;b then the clause</p>"],
  ["a table header before its delimiter row", "Before.\n| a | b |\n| --", "<p>Before.</p>"],
  ["a lone list marker", "Before.\n- ", "<p>Before.</p>"],
  ["a lone heading marker", "Before.\n##", "<p>Before.</p>"],
  ["a fence's first backticks", "Before.\n``", "<p>Before.</p>"],
  [
    "five open runs: the innermost four close, the outer one stays text",
    "*a _b ~~c **d *e f",
    "<p>*a <em>b <del>c <strong>d <em>e f</em></strong></del></em></p>",
  ],
])("the streaming tail with %s", (_, text, html) => {
  expect(tail(text)).toBe(html);
});

test.each([
  ["a fenced code line", "```text\ncode\n| piped code line", "| piped code line"],
  ["an indented code line", "Before.\n\n    | indented code", "| indented code"],
  ["a line of an HTML block", "<div>\n| in html", "| in html"],
  ["a code line in a list item", "- ```\n  | in code", "| in code"],
  ["a lone marker in code", "```\ncode\n-", "code\n-"],
  ["a trailing backslash in code", "```\nC:\\", "C:\\"],
])("the streaming tail shows %s as it is: no table or marker forms there", (_, text, shown) => {
  expect(tail(text)).toContain(shown);
});

test.each([
  ["opens a paragraph", "Before.\n\n| a | b |", ""],
  ["continues a paragraph", "Before.\n| a | b |", "<p>Before.</p>"],
  ["opens a block quote", "> | quoted line", ""],
  [
    "continues a quoted paragraph",
    "> quoted\n> | line",
    "<blockquote>\n<p>quoted</p>\n</blockquote>",
  ],
  ["is in a list item", "- item\n  | a |", "<ul>\n<li>item</li>\n</ul>"],
])(
  "a pipe-led last line that %s may be a table's header: hidden until the next line",
  (_, text, html) => {
    expect(tail(text)).toBe(html);
  },
);

test("only the innermost four openers get closers: closing all of `*a `×2,000 made the parse quadratic", () => {
  // The hostile cases no longer catch this alone (nesting.ts's limits keep them under 8 times prose), so
  // the limit is pinned here: four is every nesting prose needs (see MAX_CLOSERS).
  expect(MAX_CLOSERS).toBe(4);
  const input = `x ${"*a ".repeat(2000)}`;
  let parses = 0;
  const repaired = repairTail(input, 0, (text) => {
    parses++;
    return parse(text);
  });
  // The closers go after the last `a`, before the trailing space; the text before them is as written.
  expect(repaired).toBe(`${input.trimEnd()}${"*".repeat(MAX_CLOSERS)} `);
  expect(parses).toBe(1);
  expect(tail(input).match(/<em>/g)).toHaveLength(MAX_CLOSERS);
});

test("a run nesting.ts cut short gets no closer: a signature line's underscores stay underscores", () => {
  const html = tail(`Signature: ${"_".repeat(70)}`);
  expect(html).toBe(`<p>Signature: ${"_".repeat(70)}</p>`);
  expect(tail(`*Signed: ${"_".repeat(70)}`)).toBe(`<p><em>Signed: ${"_".repeat(70)}</em></p>`);
});

test("a repair never makes a link the raw text does not have: [a][b] with only a defined", () => {
  // The raw text `[a][b]` is literal (a label follows `[a]`); cutting `[b]`'s brackets alone would leave
  // `[a]b`, a shortcut link the finished text never shows.
  for (const tailText of ["see [a][b]", "see [a][b", "see **[a][b]"]) {
    const html = tail(`[a]: https://example.com\n\n${tailText}`);
    expect(html, tailText).not.toContain("<a");
    expect(html).not.toMatch(/[[\]]/);
  }
  expect(tail("[a]: https://example.com\n\nsee [a] and")).toContain("<a");
});

test("a half link is never a link, and a link appears only once its destination is complete", () => {
  const text = "See [the Act](https://indiacode.nic.in/act) for the rest.";
  for (let end = 1; end <= text.length; end++) {
    const html = tail(text.slice(0, end));
    expect(html.includes("<a"), html).toBe(end >= text.indexOf(")") + 1);
    expect(html).not.toMatch(/\[|\]\(/);
  }
});

test("finished text keeps its markers: tail repair is for the streaming last block only", () => {
  const html = htmlOf(createAnswer("t").update("The **key and see [the Act", false));
  expect(html).toBe("<p>The **key and see [the Act</p>");
});

test("a repaired tail needs no second repair, for every prefix of the spec examples and the legal answer", () => {
  const split = createSplitter((block) => endOf(block, parse(block)));
  let repaired = 0;
  for (const text of [LEGAL, ...examplesOf(spec).map((example) => example.markdown)]) {
    const step = text === LEGAL ? 5 : 1;
    for (let end = 1; end <= text.length; end += step) {
      const block = split(text.slice(0, end)).at(-1)!;
      const once = repairTail(block, 0, parse);
      if (once === undefined) continue;
      repaired++;
      expect(repairTail(once, 0, parse), JSON.stringify(block)).toBeUndefined();
    }
  }
  expect(repaired).toBeGreaterThan(3000);
}, 60_000);
