// pnpm test (tests/caps.test.mjs): the global daily caps, against a fresh local D1. cap + 1
// valid requests: the first cap are stored, the last is 429, stores nothing and leaves the count
// at the cap.

import { after, before, test } from "node:test";
import assert from "node:assert/strict";
import { dayJson, day, startWorker, tallyJson, uuid } from "./harness.mjs";

let worker;
before(async () => {
  worker = await startWorker();
});
after(async () => {
  await worker?.stop();
});

/**
 * Sends `cap` valid requests and one more; true when the UTC day did not turn while it ran (the
 * count starts again each day, so a turn makes the run say nothing).
 */
async function fill(path, cap, body, last) {
  const today = day(0);
  for (let i = 1; i <= cap; i += 1) {
    const status = (await worker.post(path, body(i))).status;
    if (status !== 204) {
      if (day(0) !== today) return false;
      assert.fail(`request ${i} of ${cap} on ${path}: ${status}`);
    }
  }
  const response = await worker.post(path, last);
  if (day(0) !== today) return false;
  assert.equal(response.status, 429, `request ${cap + 1} on ${path}`);
  assert.equal(await response.text(), "");
  return true;
}

test("the 3,001st counts request in a day is 429 and stores nothing", async (t) => {
  const install = uuid();
  const last = uuid();
  const counted = await fill("/v1/counts", 3_000, () => ({ days: [dayJson({ install })] }), { days: [dayJson({ install: last })] });
  if (!counted) return t.skip("the UTC day turned while it ran");
  assert.equal(worker.sql(`SELECT COUNT(*) AS n FROM report WHERE install = '${last}'`)[0].n, 0);
  assert.deepEqual(worker.sql("SELECT day, n FROM rate WHERE route = 'counts'"), [{ day: day(0), n: 3_000 }]);
});

test("the 1,001st tally in a day is 429 and stores nothing", async (t) => {
  const last = uuid();
  const counted = await fill("/v1/tally", 1_000, () => tallyJson(), tallyJson({ tally: last }));
  if (!counted) return t.skip("the UTC day turned while it ran");
  assert.equal(worker.sql(`SELECT COUNT(*) AS n FROM tally WHERE tally = '${last}'`)[0].n, 0);
  assert.equal(worker.sql("SELECT COUNT(*) AS n FROM tally")[0].n, 1_000);
  assert.deepEqual(worker.sql("SELECT n FROM rate WHERE route = 'tally'"), [{ n: 1_000 }]);
});
