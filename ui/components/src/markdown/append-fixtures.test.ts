// The append fast path (append.ts) over the corpora: the CommonMark and GFM spec examples and the legal answers,
// streamed at random deltas (the hostile cases are append-hostile.test.ts's). Each spec example is checked
// against a fresh answer at every update; the legal answers against the same answer with the fast path off at
// every update, and against a fresh answer at a sample of updates and once finished.
import { expect, test } from "vitest";
import spec from "commonmark-spec/spec.txt?raw";
import gfmSpec from "./gfm-spec.json";
import { againstFresh, againstSlow, deltas, newTotals, randomOf } from "./append-fixture.ts";
import { loadEntities } from "./entities.ts";
import { LEGAL_EXTRAS } from "./legal-extras-fixture.ts";
import { LEGAL } from "./legal-fixture.ts";
import { examplesOf, withTabs } from "./spec-fixture.ts";

await loadEntities();

test("every spec example streams, at every update, as a fresh answer renders it", () => {
  const random = randomOf(652);
  const totals = newTotals();
  const examples = [...examplesOf(spec), ...gfmSpec.examples.map(withTabs)];
  for (const { markdown } of examples)
    againstFresh(markdown, deltas(random, markdown.length, 7), totals);
  console.log(`append spec: ${totals.fastUpdates} of ${totals.updates} updates took the fast path`);
  expect(totals.fastUpdates).toBeGreaterThan(0);
}, 120_000);

test("the legal answers stream with the fast path as without it, and as a fresh answer", () => {
  const random = randomOf(57);
  const totals = newTotals();
  for (const text of [LEGAL, LEGAL_EXTRAS])
    againstSlow(text, deltas(random, text.length, 93), totals, 25);
  console.log(
    `append legal: ${totals.fastUpdates} of ${totals.updates} updates took the fast path`,
  );
  expect(totals.fastUpdates).toBeGreaterThan(totals.updates / 4);
}, 120_000);
