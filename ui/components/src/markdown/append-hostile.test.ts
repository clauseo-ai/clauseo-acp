// The append fast path (append.ts) over every hostile case, streamed at random deltas: each mdast the fast path
// makes is checked against micromark's as it is made, and the views against a fresh answer at a sample of
// updates and once finished. The cases are pathological on purpose (540 KB; two answers compared at every
// update took 40 s), so a long case streams in larger deltas, about 40 of them.
import { expect, test } from "vitest";
import { appendProbe } from "./append.ts";
import { againstSampled, deltas, newTotals, randomOf } from "./append-fixture.ts";
import { loadEntities } from "./entities.ts";
import { CASES } from "./hostile-cases.ts";
import { parse } from "./parse.ts";

await loadEntities();

test("every hostile case streams with every fast-path mdast micromark's, and renders as a fresh answer", () => {
  const random = randomOf(540);
  const totals = newTotals();
  let checked = 0;
  appendProbe.check = (source, root) => {
    expect(root, JSON.stringify(source.slice(-80))).toEqual(parse(source));
    checked++;
  };
  try {
    for (const { markdown, delta } of CASES) {
      // A long case in about 40 updates.
      const most = Math.max(delta ?? 93, Math.ceil(markdown.length / 20));
      const sizes = deltas(
        random,
        markdown.length,
        delta && markdown.length < 4_000 ? delta : most,
      );
      againstSampled(markdown, sizes, totals, Math.max(1, Math.floor(sizes.length / 4)));
    }
  } finally {
    appendProbe.check = undefined;
  }
  expect(checked).toBe(totals.fast);
  console.log(
    `append hostile: ${totals.fastUpdates} of ${totals.updates} updates took the fast path`,
  );
  expect(totals.fastUpdates).toBeGreaterThan(0);
}, 300_000);
