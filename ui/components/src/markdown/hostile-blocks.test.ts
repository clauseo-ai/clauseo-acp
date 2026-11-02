import type { Nodes as Hast } from "hast";
import { expect, test } from "vitest";
import { createAnswer } from "./answer.ts";
import { CASES } from "./hostile-cases.ts";
import { TIME_FILES, timeSlice } from "./hostile-time-fixture.ts";
import { MAX_JOINED, MAX_PARAGRAPH } from "./split.ts";

/**
 * The hostile cases whose blocks may be longer than MAX_PARAGRAPH, and why: no cut inside them is safe.
 * capNesting bounds the depth of each, and each passed the real engines' stall guard.
 */
const LONG_BLOCKS: Record<string, string> = {
  "deep-quote": "one block quote, nested as deep as nesting.ts allows",
  "tab-nesting": "one list, nested through tabs as deep as nesting.ts allows",
  "footnote-many-labels": "a footnote's note stays one block",
  "footnote-deeper-list": "a footnote's note stays one block",
  "extras-definition-markers": "a definition list is never cut",
};
/** The elements the streaming last block of a hostile case may hold. */
const MAX_ELEMENTS = 1_000;

const elementsIn = (node: Hast): number =>
  (node.type === "element" ? 1 : 0) +
  ("children" in node ? node.children.reduce((sum, child) => sum + elementsIn(child), 0) : 0);

// What the ratio test (hostile-time-fixture.ts) cannot see. Each update parses and renders the streaming last block whole, and in
// a real engine its flush costs that block's parse, view, React render and commit. The prose it is compared with is one paragraph of the same length, parsed whole on
// every update too, so it grows with the case: `"a==b==".repeat(3000)`, one 18 KB paragraph, cost about
// 1.7 times its prose in Node and passed, while in WebView2 on 2 vCPUs one of its flushes took 1.2 s (and
// finishing it 1.5 s), and `"^a^".repeat(6000)` made 6,000 `<sup>` in one block. Bounding every block's
// length and elements bounds what one update parses and renders, whatever the engine; the split is
// incremental and its cuts never move (split.test.ts), so the streaming last block is bounded too. A
// paragraph's parts are joined once it is finished, up to MAX_JOINED (split.ts): that update parses and
// renders it once, which the ratio test times (case links-joined-paragraph is just under it).
test.each(CASES.map((c) => [c.id, c] as const))(
  `%s: the streaming last block holds at most ${MAX_PARAGRAPH} characters and ${MAX_ELEMENTS} elements, every block at most ${MAX_JOINED} characters`,
  (id, c) => {
    for (const streaming of [true, false]) {
      const views = createAnswer("b").update(c.markdown, streaming);
      const blocks = views.filter((view) => !view.items);
      for (const view of blocks) {
        const tail = streaming && view === blocks.at(-1);
        if (!(id in LONG_BLOCKS))
          expect(view.source.length, `${id} (streaming ${streaming})`).toBeLessThanOrEqual(
            tail ? MAX_PARAGRAPH : MAX_JOINED,
          );
        if (tail)
          expect(elementsIn(view.hast), `${id} (streaming ${streaming})`).toBeLessThanOrEqual(
            MAX_ELEMENTS,
          );
      }
    }
  },
  30_000,
);

test("links-joined-paragraph: cut while it streams, one block once the next block starts", () => {
  const { markdown } = CASES.find((c) => c.id === "links-joined-paragraph")!;
  const paragraph = markdown.slice(0, markdown.indexOf("\n\n") + 2);
  expect(paragraph.length).toBeGreaterThan(MAX_JOINED - 512);
  expect(paragraph.length).toBeLessThanOrEqual(MAX_JOINED);
  const answer = createAnswer("j");
  expect(answer.update(paragraph, true).length).toBeGreaterThan(3);
  const views = answer.update(`${paragraph}T`, true);
  expect(views.map((view) => view.source.length)).toEqual([paragraph.length, 1]);
});

test("the timed files between them time every case once", () => {
  const timed = Array.from({ length: TIME_FILES }, (_, k) => timeSlice(k + 1)).flat();
  expect(timed.map((c) => c.id).sort()).toEqual(CASES.map((c) => c.id).sort());
  const files = Object.keys(import.meta.glob("./hostile-time-*.timed.test.ts"));
  expect([...files].sort()).toEqual(
    Array.from({ length: TIME_FILES }, (_, k) => `./hostile-time-${k + 1}.timed.test.ts`),
  );
});
