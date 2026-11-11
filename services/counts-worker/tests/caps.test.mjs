// pnpm test (tests/caps.test.mjs): the global hourly caps, against a fresh local D1. cap + 1
// valid requests: the first cap are stored, the last is 429 and stores nothing.

import { after, before, test } from "node:test";
import assert from "node:assert/strict";
import { dayJson, startWorker, tallyJson, uuid } from "./harness.mjs";

let worker;
before(async () => {
  worker = await startWorker();
});
after(async () => {
  await worker?.stop();
});

const hourNow = () => Math.floor(Date.now() / 3_600_000);

/**
 * Sends `cap` valid requests and one more; true when the hour did not turn while it ran (the
 * count starts again each hour, so a turn makes the run say nothing).
 */
async function fill(path, cap, body, last) {
  const hour = hourNow();
  for (let i = 1; i <= cap; i += 1) {
    const status = (await worker.post(path, body(i))).status;
    if (status !== 204) {
      if (hourNow() !== hour) return false;
      assert.fail(`request ${i} of ${cap} on ${path}: ${status}`);
    }
  }
  const response = await worker.post(path, last);
  if (hourNow() !== hour) return false;
  assert.equal(response.status, 429, `request ${cap + 1} on ${path}`);
  assert.equal(await response.text(), "");
  return true;
}

test("the 251st counts request in an hour is 429 and stores nothing", async () => {
  const install = uuid();
  const last = uuid();
  const counted = await fill("/v1/counts", 250, () => ({ days: [dayJson({ install })] }), { days: [dayJson({ install: last })] });
  if (!counted) return;
  assert.equal(worker.sql(`SELECT COUNT(*) AS n FROM report WHERE install = '${last}'`)[0].n, 0);
  assert.equal(worker.sql("SELECT n FROM rate WHERE route = 'counts'")[0].n, 251);
});

test("the 101st tally in an hour is 429 and stores nothing", async () => {
  const last = uuid();
  const counted = await fill("/v1/tally", 100, () => tallyJson(), tallyJson({ tally: last }));
  if (!counted) return;
  assert.equal(worker.sql(`SELECT COUNT(*) AS n FROM tally WHERE tally = '${last}'`)[0].n, 0);
  assert.equal(worker.sql("SELECT COUNT(*) AS n FROM tally")[0].n, 100);
});
