import { expect, test } from "vitest";
import { createAnswer } from "./answer.ts";
import type { BlockView } from "./answer.ts";
import { labelsIn } from "./definitions.ts";
import { htmlOf, normalise, unsplit } from "./spec-fixture.ts";

// Each block gets only the definitions whose labels it uses (PR #107's fidelity review: one definition
// arriving at the end gave every bracketed block a new view, and 40 definitions first made every update
// parse all 40 again).

const paragraph = (index: number) =>
  `Paragraph ${index} cites [source ${(index % 50) + 1}] for the point, as advice does.\n\n`;
const definition = (n: number) => `[source ${n}]: https://example.com/source/${n}\n\n`;
const PARAGRAPHS = Array.from({ length: 150 }, (_, index) => paragraph(index)).join("");
const definitions = (count: number) =>
  Array.from({ length: count }, (_, index) => definition(index + 1)).join("");

/** Every update of `text` streamed in 16-character deltas, then the final one. */
function stream(text: string): (readonly BlockView[])[] {
  const answer = createAnswer("t");
  const updates: (readonly BlockView[])[] = [];
  for (let end = 16; end < text.length + 16; end += 16)
    updates.push(answer.update(text.slice(0, Math.min(end, text.length)), true));
  updates.push(answer.update(text, false));
  return updates;
}

test.each([2, 40])(
  "150 citing paragraphs, then %i definitions: a definition re-renders only the paragraphs citing it",
  (count) => {
    const updates = stream(PARAGRAPHS + definitions(count));
    const renewed = Array.from({ length: 150 }, () => 0);
    for (let at = 1; at < updates.length; at++) {
      const [before, after] = [updates[at - 1]!, updates[at]!];
      const changed: number[] = [];
      // A paragraph finished in both updates (the streaming last block changes on every delta).
      for (let index = 0; index < 150 && index < before.length - 1; index++)
        if (after[index] !== before[index]) changed.push(index);
      for (const index of changed) renewed[index]!++;
      // One definition arrives at a time: the 3 paragraphs that cite its label, and no others.
      expect(changed.length).toBeLessThanOrEqual(3);
      expect(new Set(changed.map((index) => index % 50)).size).toBeLessThanOrEqual(1);
    }
    // Each cited source's paragraphs got exactly one new view, when its definition arrived.
    expect(renewed).toEqual(renewed.map((_, index) => (index % 50 < count ? 1 : 0)));
    const final = htmlOf(updates.at(-1)!);
    expect(final).toContain('<a href="https://example.com/source/1">source 1</a>');
  },
);

test("40 definitions first: every block parses only the definitions it uses", () => {
  const text = definitions(40) + PARAGRAPHS;
  let checked = 0;
  for (const views of stream(text)) {
    for (const view of views) {
      if (!view.source.startsWith("Paragraph")) continue;
      // At most the one definition it cites (none while its label is still arriving), then the block.
      const cited = /\[(source \d+)/.exec(view.source)?.[1];
      const lines = view.input.match(/^\[source \d+\]: .*$/gm) ?? [];
      expect(lines.length).toBeLessThanOrEqual(1);
      if (lines[0]) expect(lines[0].startsWith(`[${cited}]: `)).toBe(true);
      checked++;
    }
  }
  expect(checked).toBeGreaterThan(1000);
  expect(normalise(htmlOf(createAnswer("t").update(text, false)))).toBe(normalise(unsplit(text)));
});

test.each([
  ["a label that case folding grows past 999 characters", `a [b] c\n\n[${"ß".repeat(500)}]: /u\n`],
  ["a reference to it", `[${"ß".repeat(500)}] c\n\n[${"ß".repeat(500)}]: /u\n`],
  ["a label over two lines in a block quote", "> See [foo\n> bar] here.\n\n[foo bar]: /u\n"],
  ["a definition over two lines in a block quote", "[foo bar]\n\n> [foo\n> bar]: /u\n"],
  ["an escaped bracket in a label", "[a\\]b] and [a\\[c]\n\n[a\\]b]: /u\n\n[a\\[c]: /v\n"],
  ["a character reference in a label", "[&amp;] and [&]\n\n[&amp;]: /u\n\n[&]: /v\n"],
  ["a backslash ending a code span before a reference", "`x\\`[foo]\n\n[foo]: /u\n"],
  ["a full and a collapsed reference", "[text][Foo] and [Foo][]\n\n[foo]: /u\n"],
  ["a reference inside link text", "[a [b] c][d]\n\n[b]: /b\n\n[d]: /d\n"],
])("block by block renders as in one piece: %s", (_, text) => {
  expect(normalise(htmlOf(createAnswer("t").update(text, false)))).toBe(normalise(unsplit(text)));
});

test("a block's labels: every innermost bracket pair, normalised as micromark matches them", () => {
  expect(labelsIn("[Foo  Bar] and [x [y] z][Ref]")).toEqual(["FOO BAR", "Y", "REF"]);
  expect(labelsIn("> [foo\n> bar]")).toEqual(["FOO > BAR", "FOO BAR"]);
  expect(labelsIn("[a\\]b]")).toEqual(["A\\]B", "A\\"]);
  expect(labelsIn("no brackets")).toEqual([]);
});
