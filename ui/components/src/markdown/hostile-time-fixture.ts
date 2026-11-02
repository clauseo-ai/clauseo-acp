import { expect, test } from "vitest";
import { createAnswer } from "./answer.ts";
import { CASES } from "./hostile-cases.ts";
import type { HostileCase } from "./hostile-cases.ts";

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
//
// Each hostile-time-<k>.timed.test.ts times every TIME_FILES-th case (timeSlice). The *.timed.test.ts files are
// vite.config.ts's components-timed project, which runs before any other test file starts, with at most three
// files at once: on a 4-vCPU CI runner the timed cases share the machine with each other only, never with the DOM
// tests, which is less than they shared it with when this was one file beside them all. The ratio, the floor and
// the rounds above are the margin; none is loosened by the split.
const RATIO = 8;
const FLOOR_MS = 20;
const DELTA = 16;
/** Interleaved rounds of timing; each figure is the least of its rounds. */
const ROUNDS = 4;
/** The slowest deltas of the first streaming pass that the rounds time again. */
const WORST = 2;
const PROSE =
  "Lorem ipsum dolor sit amet, **consectetur** adipiscing elit, sed do _eiusmod_ tempor. ";

/** The prefix lengths a case streams through, as in hostile-dom-fixture.tsx: every delta, or both ends of a long case. */
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

/** How many hostile-time-<k>.timed.test.ts files there are; file k times the cases at k-1, k-1 + TIME_FILES, ... */
export const TIME_FILES = 3;

/** The cases hostile-time-<file>.timed.test.ts times (file counts from 1). */
export function timeSlice(file: number): HostileCase[] {
  return CASES.filter((_, index) => index % TIME_FILES === file - 1);
}

/** Registers the ratio test of every case in timeSlice(file). */
export function timeCases(file: number): void {
  test.each(timeSlice(file).map((c) => [c.id, c] as const))(
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
      // Under the floor in round one, a case passes whatever the later rounds find: they can only lower its
      // figures, and the limit never goes below FLOOR_MS. So they run only for a case at or above the floor.
      const settled = () => Math.max(finished, ...worst.map(({ least }) => least)) < FLOOR_MS;
      for (let round = 1; round < ROUNDS && !settled(); round++) {
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
}
