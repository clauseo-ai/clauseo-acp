import { expect, test } from "vitest";
import { createAnswer } from "./answer.ts";
import { CASES } from "./hostile-cases.ts";

// No hostile case may freeze the answer. One update of the model (split, definitions, parse, sanitise)
// may cost at most RATIO times what one update of a single paragraph of ordinary prose of the same length
// costs, measured in the same run (so a loaded machine slows both), and at least FLOOR_MS is allowed (tiny
// cases are all noise). Checked finished in one go and at every 16-character delta while streaming.
//
// Why 8 times: without the limits in nesting.ts, the cases this guards against cost about 28 to 140 times
// the prose baseline in Node (round 2 of PR #107's review: 10 KB of nesting through tabs 2.2 s, 5,000 `*`
// a side 1.4 s, 5,000 short runs 0.56 s), while every case now stays within about 3.5 times (the
// 20,000-character line, and the tab-nested list whose depth micromark checks on every line). An absolute limit in milliseconds fails on a loaded CI machine and says nothing
// about size; a frame (16.7 ms) is the real target, measured in real engines by the private harness.
const RATIO = 8;
const FLOOR_MS = 20;
const DELTA = 16;
const PROSE =
  "Lorem ipsum dolor sit amet, **consectetur** adipiscing elit, sed do _eiusmod_ tempor. ";

/** The prefix lengths a case streams through, as in hostile.test.tsx: every delta, or both ends of a long case. */
function ends(length: number): number[] {
  const all: number[] = [];
  for (let end = DELTA; end < length + DELTA; end += DELTA) all.push(Math.min(end, length));
  return length <= 2_000 ? all : all.filter((end) => end <= 1_000 || end >= length - 640);
}

/** The fastest of three timings of `update`, so one scheduler hiccup does not count. */
function fastest(update: () => void): number {
  const times: number[] = [];
  for (let attempt = 0; attempt < 3; attempt++) {
    const start = performance.now();
    update();
    times.push(performance.now() - start);
  }
  return Math.min(...times);
}

test.each(CASES.map((c) => [c.id, c] as const))(
  "%s: no update costs more than 8 times as much as ordinary prose of its length",
  (_, c) => {
    const prose = PROSE.repeat(Math.ceil(c.markdown.length / PROSE.length)).slice(
      0,
      c.markdown.length,
    );
    const limit = Math.max(FLOOR_MS, RATIO * fastest(() => createAnswer("p").update(prose, false)));
    const finished = fastest(() => createAnswer("t").update(c.markdown, false));
    let streaming = 0;
    const answer = createAnswer("s");
    for (const end of ends(c.markdown.length)) {
      const start = performance.now();
      answer.update(c.markdown.slice(0, end), true);
      const took = performance.now() - start;
      // Over the limit: measured again from a fresh model at that length, to tell a hiccup from a slow update.
      streaming = Math.max(
        streaming,
        took < limit
          ? took
          : fastest(() => createAnswer("r").update(c.markdown.slice(0, end), true)),
      );
    }
    expect(finished, "finished").toBeLessThan(limit);
    expect(streaming, "streaming").toBeLessThan(limit);
  },
  60_000,
);
