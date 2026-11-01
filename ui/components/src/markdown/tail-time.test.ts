import { expect, test } from "vitest";
import { createAnswer } from "./answer.ts";
import { CASES } from "./hostile-cases.ts";

// The hostile cases of tail repair and footnotes may not freeze the answer, by PR #107's rule (its
// hostile-time.test.ts, which covers every case once both are on main): one update of the model may cost
// at most RATIO times one update of ordinary prose of the same length, measured in the same run (a loaded
// machine slows both), and at least FLOOR_MS (tiny cases are all noise). Checked finished, and at every
// 16-character delta while streaming, since tail repair runs only then. One difference: an update over the
// limit is timed again next to the prose baseline timed again, since this suite runs beside the others.
//
// Before the repair's limits, one streaming update of `*a `×4,000 took 6.3 s and of `_a `×4,000 7.5 s
// (prose of the same length: 7.5 ms, so about 900 times), and of `[`×16,000 0.66 s (about 50 times). Now
// they stay within about 3, 5 and 6 times; the brackets' cost is mostly their parse, finished as well.
const RATIO = 8;
const FLOOR_MS = 20;
const DELTA = 16;
const PROSE =
  "Lorem ipsum dolor sit amet, **consectetur** adipiscing elit, sed do _eiusmod_ tempor. ";
const IDS = new Set([
  "half-tag-mid-stream",
  "open-tag-mid-stream",
  "open-star-runs",
  "open-underscore-runs",
  "open-brackets",
  "footnote-many-labels",
  "footnote-many-notes",
  "footnote-long-label",
  "footnote-unclosed",
  "footnote-chain",
]);

/** The prefix lengths a case streams through: every delta, or both ends of a long case. */
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

test("every case this test names is in the corpus", () => {
  expect(CASES.filter((c) => IDS.has(c.id))).toHaveLength(IDS.size);
});

test.each(CASES.filter((c) => IDS.has(c.id)).map((c) => [c.id, c] as const))(
  "%s: no update costs more than 8 times as much as ordinary prose of its length",
  (_, c) => {
    const prose = PROSE.repeat(Math.ceil(c.markdown.length / PROSE.length)).slice(
      0,
      c.markdown.length,
    );
    const limitNow = () =>
      Math.max(FLOOR_MS, RATIO * fastest(() => createAnswer("p").update(prose, false)));
    const limit = limitNow();
    /**
     * How far over the limit `update` is (under 1 passes). An update over it is measured again (the
     * fastest of three), next to the prose baseline measured again, so a stretch of load on a busy
     * machine, which slows both, does not count, and a slow update still does.
     */
    const over = (took: number, update: () => void) => {
      if (took < limit) return took / limit;
      const again = fastest(update);
      return again / Math.max(limit, limitNow());
    };
    const whole = () => createAnswer("t").update(c.markdown, false);
    const finished = over(fastest(whole), whole);
    let streaming = 0;
    const answer = createAnswer("s");
    for (const end of ends(c.markdown.length)) {
      const begin = performance.now();
      answer.update(c.markdown.slice(0, end), true);
      const took = performance.now() - begin;
      const update = () => createAnswer("r").update(c.markdown.slice(0, end), true);
      streaming = Math.max(streaming, over(took, update));
    }
    expect(finished, "finished, as a share of the limit").toBeLessThan(1);
    expect(streaming, "streaming, as a share of the limit").toBeLessThan(1);
  },
  60_000,
);
