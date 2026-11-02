import { getHeapStatistics, setFlagsFromString } from "node:v8";
import { runInNewContext } from "node:vm";
import { expect, test } from "vitest";
import { createAnswer } from "./answer.ts";
import type { BlockView } from "./answer.ts";

// A streamed answer keeps no more than the same answer rendered once, give or take the caches' bounded
// leftovers. Each finished block used to be a slice of the update's whole text, so an answer of N blocks
// kept up to N old copies of itself alive: two 40 KB answers streamed as the speed scoreboard's session
// streams them kept 7 to 9 MB more than the same answers rendered once (heap after GC, Node), now about
// 1 MB. Here, 40 KB of prose in 300 paragraphs at 16-character deltas kept about 14 MB more before, and now
// keeps less than the render in one go (which keeps every block's parse); the limit is 3 MB, well clear of
// both.

setFlagsFromString("--expose-gc");
const gc = runInNewContext("gc") as () => void;

/** The heap in use after a full collection. */
function heapAfterGc(): number {
  gc();
  gc();
  return getHeapStatistics().used_heap_size;
}

const TEXT = Array.from(
  { length: 300 },
  (_, n) =>
    `Paragraph ${n} of the answer, with *emphasis* and \`code\`, runs on for a while so that it ` +
    `takes more than one delta to arrive.\n\n`,
).join("");

/** The heap the answer's model and its views keep, once the answer has ended. */
function retainedBy(streamed: boolean): number {
  const before = heapAfterGc();
  const answer = createAnswer("retain");
  // Appended to as the transcript does, so each update's text is a string of its own, not a slice of TEXT.
  let text = "";
  if (streamed)
    for (let at = 0; at < TEXT.length; at += 16)
      answer.update((text += TEXT.slice(at, at + 16)), true);
  const views: readonly BlockView[] = answer.update(streamed ? text : TEXT, false);
  const retained = heapAfterGc() - before;
  // Still in use after the measurement, so the collector could not take it before.
  expect(answer.update(TEXT, false)).toBe(views);
  expect(views).toHaveLength(300);
  return retained;
}

test("a streamed answer keeps no old copies of its text", () => {
  // The first answer also pays for what loads and compiles on first use.
  retainedBy(true);
  const once = retainedBy(false);
  const streamed = retainedBy(true);
  expect(streamed - once).toBeLessThan(3_000_000);
  // Three answers, one of them streamed through about 2,600 deltas: about 3.5 s alone, past the default 5 s with
  // three suites and a build beside it. The heap after a full collection does not depend on the load.
}, 30_000);
