import type { Nodes as Hast } from "hast";
import { expect, test } from "vitest";
import { createAnswer } from "./answer.ts";
import { CASES } from "./hostile-cases.ts";
import { MAX_PARAGRAPH } from "./split.ts";

// No hostile case may freeze the answer. One update of the model (split, definitions, parse, sanitise)
// may cost at most RATIO times what one update of a single paragraph of ordinary prose of the same length
// costs, measured in the same run (so a loaded machine slows both), and at least FLOOR_MS is allowed (tiny
// cases are all noise). Checked finished in one go and at every 16-character delta while streaming.
//
// Why 8 times: without the limits in nesting.ts, the cases this guards against cost about 28 to 140 times
// the prose baseline in Node (round 2 of PR #107's review: 10 KB of nesting through tabs 2.2 s, 5,000 `*`
// a side 1.4 s, 5,000 short runs 0.56 s), while every case above the floor now stays within about 5.6
// times (extras-definitions; long-line and extras-superscripts-6000 about 4 to 5), quiet or at load 10.
// An absolute limit in milliseconds fails on a loaded CI machine and says nothing about size; a frame
// (16.7 ms) is the real target, measured in real engines by the private harness.
//
// Times are this thread's CPU time, not wall time: with other suites running at once (load about 10),
// wall time counted the time the thread waited for a core, and 4 of 6 runs failed (round 1 of PR #115's
// review). Even CPU time grows on a busy machine (shared caches and cores, clock changes), and one timing of
// the prose against one of the case still failed in CI (footnote-many-labels, 43 ms against 40 ms). So the
// prose, the finished case and the slowest streamed deltas are each timed in ROUNDS interleaved rounds and
// the least of each is compared: a spike inflates a round, not all of them, and a real freeze is slow in
// every round.
const RATIO = 8;
const FLOOR_MS = 20;
const DELTA = 16;
/** Interleaved rounds of timing; each figure is the least of its rounds. */
const ROUNDS = 4;
/** The slowest deltas of the first streaming pass that the rounds time again. */
const WORST = 2;
const PROSE =
  "Lorem ipsum dolor sit amet, **consectetur** adipiscing elit, sed do _eiusmod_ tempor. ";

/** The prefix lengths a case streams through, as in hostile.test.tsx: every delta, or both ends of a long case. */
function ends(length: number): number[] {
  const all: number[] = [];
  for (let end = DELTA; end < length + DELTA; end += DELTA) all.push(Math.min(end, length));
  return length <= 2_000 ? all : all.filter((end) => end <= 1_000 || end >= length - 640);
}

/** This thread's CPU time so far, in milliseconds. */
function now(): number {
  const { user, system } = process.threadCpuUsage();
  return (user + system) / 1000;
}

/** The CPU time one call of `update` takes. */
function time(update: () => void): number {
  const start = now();
  update();
  return now() - start;
}

/** The CPU time of the streamed update to `end`, by a model that has just been updated to `previous`. */
function streamedTo(markdown: string, previous: number, end: number): number {
  const answer = createAnswer("r");
  if (previous > 0) answer.update(markdown.slice(0, previous), true);
  return time(() => answer.update(markdown.slice(0, end), true));
}

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
/** The elements one block of a hostile case may hold. */
const MAX_ELEMENTS = 1_000;

const elementsIn = (node: Hast): number =>
  (node.type === "element" ? 1 : 0) +
  ("children" in node ? node.children.reduce((sum, child) => sum + elementsIn(child), 0) : 0);

// What the test above cannot see. Each update parses and renders the streaming last block whole (and
// the block that has just finished), and in a real engine its flush costs that block's parse, view, React
// render and commit. The prose it is compared with is one paragraph of the same length, parsed whole on
// every update too, so it grows with the case: `"a==b==".repeat(3000)`, one 18 KB paragraph, cost about
// 1.7 times its prose in Node and passed, while in WebView2 on 2 vCPUs one of its flushes took 1.2 s (and
// finishing it 1.5 s), and `"^a^".repeat(6000)` made 6,000 `<sup>` in one block. Bounding every block's
// length and elements bounds what one update parses and renders, whatever the engine; the split is
// incremental and its cuts never move (split.test.ts), so the streaming last block is bounded too.
test.each(CASES.map((c) => [c.id, c] as const))(
  `%s: every block holds at most ${MAX_PARAGRAPH} characters and ${MAX_ELEMENTS} elements`,
  (id, c) => {
    for (const streaming of [true, false]) {
      const views = createAnswer("b").update(c.markdown, streaming);
      for (const view of views) {
        if (!(id in LONG_BLOCKS))
          expect(view.source.length, `${id} (streaming ${streaming})`).toBeLessThanOrEqual(
            MAX_PARAGRAPH,
          );
        expect(elementsIn(view.hast), `${id} (streaming ${streaming})`).toBeLessThanOrEqual(
          MAX_ELEMENTS,
        );
      }
    }
  },
  30_000,
);

test.each(CASES.map((c) => [c.id, c] as const))(
  "%s: no update costs more than 8 times as much as ordinary prose of its length",
  (_, c) => {
    const prose = PROSE.repeat(Math.ceil(c.markdown.length / PROSE.length)).slice(
      0,
      c.markdown.length,
    );
    // Round one times the prose, the finished case and every streamed delta. Each later round, interleaved,
    // times the prose, the finished case and only the slowest deltas of round one again. Each figure is the
    // least of its rounds: what the code costs, without the time a busy machine stole from any one round.
    let baseline = time(() => createAnswer("p").update(prose, false));
    let finished = time(() => createAnswer("t").update(c.markdown, false));
    const answer = createAnswer("s");
    const worst = ends(c.markdown.length)
      .map((end, i, all) => ({
        previous: all[i - 1] ?? 0,
        end,
        least: time(() => answer.update(c.markdown.slice(0, end), true)),
      }))
      .sort((x, y) => y.least - x.least)
      .slice(0, WORST);
    for (let round = 1; round < ROUNDS; round++) {
      baseline = Math.min(
        baseline,
        time(() => createAnswer("p").update(prose, false)),
      );
      finished = Math.min(
        finished,
        time(() => createAnswer("t").update(c.markdown, false)),
      );
      for (const delta of worst)
        delta.least = Math.min(delta.least, streamedTo(c.markdown, delta.previous, delta.end));
    }
    const streaming = Math.max(0, ...worst.map(({ least }) => least));
    const limit = Math.max(FLOOR_MS, RATIO * baseline);
    expect(finished, "finished").toBeLessThan(limit);
    expect(streaming, "streaming").toBeLessThan(limit);
  },
  // A guard against a hang, not a check of the cost: the largest case takes about 20 s alone and 50 s with
  // three suites and a build beside it.
  180_000,
);
