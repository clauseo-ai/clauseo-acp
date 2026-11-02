import { expect, test } from "vitest";
import { createAnswer } from "./answer.ts";
import { CASES } from "./hostile-cases.ts";
import type { HostileCase } from "./hostile-cases.ts";
import { sliceOfThisFile } from "./hostile-slice-fixture.ts";

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
// prose, the finished case and the slowest streamed deltas are timed in ROUNDS interleaved rounds, and each
// round gives its own ratios: the case's figure in that round over the prose timed in that same round. Load
// that hits a round hits both sides of its ratio, so it cancels; the median of the ROUNDS ratios (ROUNDS is
// odd, so it is one round's ratio) drops the one or two rounds a spike disturbed; and a real freeze is slow
// in every round, so its median stays over RATIO.
//
// Each hostile-time-<k>.timed.test.ts times every TIME_FILES-th case (timeSlice). The *.timed.test.ts files are
// vite.config.ts's components-timed project, which runs before any other test file starts, with at most three
// files at once: on a 4-vCPU CI runner the timed cases share the machine with each other only, never with the DOM
// tests, which is less than they shared it with when this was one file beside them all. The ratio, the floor and
// the rounds above are the margin; none is loosened by the split.
const RATIO = 8;
const FLOOR_MS = 20;
const DELTA = 16;
/**
 * Interleaved rounds of timing, odd so that the median of a case's per-round ratios is one round's ratio. A round's
 * ratio is its figure over the prose baseline of that round, the baseline never taken below FLOOR_MS / RATIO, so
 * the floor allows FLOOR_MS as it always has.
 */
const ROUNDS = 5;
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

/** A round's figure over its prose baseline; the baseline is never below FLOOR_MS / RATIO, so the floor allows FLOOR_MS. */
function ratio(figure: number, baseline: number): number {
  return figure / Math.max(baseline, FLOOR_MS / RATIO);
}

/** The middle of an odd count of ratios (ROUNDS is odd), so one round's ratio. */
function median(ratios: number[]): number {
  const sorted = [...ratios].sort((a, b) => a - b);
  return sorted[Math.floor(sorted.length / 2)] ?? 0;
}

/** The ratios, two decimals each, for a failure message. */
function listed(ratios: number[]): string {
  return `[${ratios.map((r) => r.toFixed(2)).join(", ")}]`;
}

/** The CPU time of the streamed update to `end`, by a model that has just been updated to `previous`. */
function streamedTo(markdown: string, previous: number, end: number): number {
  const answer = createAnswer("r");
  if (previous > 0) answer.update(markdown.slice(0, previous), true);
  return time(() => answer.update(markdown.slice(0, end), true));
}

/** How many hostile-time-<k>.timed.test.ts files there are; file k times the cases at k-1, k-1 + TIME_FILES, ... */
export const TIME_FILES = 3;

/** The cases hostile-time-<file>.timed.test.ts times (file counts from 1), for hostile-blocks.test.ts's guard. */
export function timeSlice(file: number): HostileCase[] {
  return CASES.filter((_, index) => index % TIME_FILES === file - 1);
}

/**
 * Registers the ratio test of every case in timeSlice(k), k from the name of the hostile-time-<k>.timed.test.ts
 * being collected (hostile-dom-fixture.tsx's sliceOfThisFile). Called with nothing, so a file can neither pick
 * another slice nor drop a case from its own.
 */
export function timeThisSlice(): void {
  const file = sliceOfThisFile("hostile-time-", ".timed.test.ts", TIME_FILES);
  test.each(timeSlice(file).map((c) => [c.id, c] as const))(
    "%s: no update costs more than 8 times as much as ordinary prose of its length",
    (_, c) => {
      const prose = PROSE.repeat(Math.ceil(c.markdown.length / PROSE.length)).slice(
        0,
        c.markdown.length,
      );
      const timeProse = () => time(() => createAnswer("p").update(prose, false));
      const timeFinished = () => time(() => createAnswer("t").update(c.markdown, false));
      // Round one times the prose, the finished case and every streamed delta, and keeps the WORST slowest deltas.
      // Each later round, interleaved, times the prose, the finished case and those deltas again. Every round's
      // figure becomes that round's ratio over the prose timed in the same round, so the figures are never mixed
      // across rounds.
      const baseline1 = timeProse();
      const finished1 = timeFinished();
      const answer = createAnswer("s");
      const worst = ends(c.markdown.length)
        .map((end, i, all) => ({
          previous: all[i - 1] ?? 0,
          end,
          figure: time(() => answer.update(c.markdown.slice(0, end), true)),
        }))
        .sort((x, y) => y.figure - x.figure)
        .slice(0, WORST)
        .map((delta) => ({ ...delta, ratios: [ratio(delta.figure, baseline1)] }));
      const finishedRatios = [ratio(finished1, baseline1)];
      // Under the floor in round one, a case passes whatever the later rounds find: a round's ratio is at most
      // its figure over FLOOR_MS / RATIO, so under the floor it is under RATIO. So they run only for a case at or
      // above the floor.
      const settled = Math.max(finished1, ...worst.map(({ figure }) => figure)) < FLOOR_MS;
      for (let round = 1; round < ROUNDS && !settled; round++) {
        const baseline = timeProse();
        finishedRatios.push(ratio(timeFinished(), baseline));
        for (const delta of worst)
          delta.ratios.push(ratio(streamedTo(c.markdown, delta.previous, delta.end), baseline));
      }
      const streaming = Math.max(0, ...worst.map(({ ratios }) => median(ratios)));
      expect(
        median(finishedRatios),
        `finished, per-round ratios ${listed(finishedRatios)}`,
      ).toBeLessThanOrEqual(RATIO);
      expect(
        streaming,
        `streaming, per-round ratios ${worst
          .map(({ end, ratios }) => `at ${end} ${listed(ratios)}`)
          .join("; ")}`,
      ).toBeLessThanOrEqual(RATIO);
    },
    // A guard against a hang, not a check of the cost: the largest case takes about 20 s alone and 50 s with
    // three suites and a build beside it.
    180_000,
  );
}
