// pnpm test (tests/crash.test.mjs): POST /v1/crash under `wrangler dev --local`, with local D1 and
// R2. The crate's example report is kept once, in R2 as its canonical JSON and in D1 as one
// row, and a repeat of its id leaves both as first stored; anything outside the schema or its
// caps is 400; past 200 reports an hour is 429, after one statement that changes nothing; and a
// Worker without the bucket bound answers 503.

import { after, before, test } from "node:test";
import assert from "node:assert/strict";
import { randomBytes } from "node:crypto";
import { readFileSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { gzipSync } from "node:zlib";
import { crashJson, root, startWorker, uuid } from "./harness.mjs";

const UNBOUND = "wrangler.test-unbound.toml";
const R2_TABLE = '[[r2_buckets]]\nbinding = "CRASH"\nbucket_name = "clauseo-crash"\njurisdiction = "eu"\n';

let worker;
before(async () => {
  worker = await startWorker();
});
after(async () => {
  await worker?.stop();
  rmSync(join(root, UNBOUND), { force: true });
});

const rows = (report) => worker.sql(`SELECT * FROM crash WHERE report = '${report}'`);
const rateTotal = () => worker.sql("SELECT COALESCE(SUM(n), 0) AS n FROM crash_rate")[0].n;
/** The current UTC hour as the Worker labels it. */
const hourNow = () => new Date().toISOString().slice(0, 13);

test("the crate's example report is kept once: its canonical JSON in R2, its row in D1", async () => {
  const report = crashJson();
  assert.equal((await worker.post("/v1/crash", report)).status, 204);
  assert.equal(await worker.object(`reports/${report.report}.json`), JSON.stringify(report));
  assert.deepEqual(rows(report.report), [
    {
      report: report.report,
      week: report.week,
      version: "3.0.4",
      os: "windows",
      arch: "x86_64",
      kind: "panic",
      area: "store",
      signature: "crates/store/src/writer.rs:212",
    },
  ]);
  // Sent again: 204, and still one row and the same object.
  assert.equal((await worker.post("/v1/crash", report)).status, 204);
  assert.equal(rows(report.report).length, 1);
});

test("a report id sent again with other fields: 204, and the object and the row stay the first", async () => {
  const first = crashJson();
  const second = crashJson({ report: first.report });
  second.after_launch = false;
  second.location.line = 213;
  assert.equal((await worker.post("/v1/crash", first)).status, 204);
  const row = rows(first.report);
  assert.equal(row.length, 1);
  assert.equal((await worker.post("/v1/crash", second)).status, 204);
  assert.equal(await worker.object(`reports/${first.report}.json`), JSON.stringify(first));
  assert.deepEqual(rows(first.report), row);
  assert.equal(row[0].signature, "crates/store/src/writer.rs:212");
});

test("the first report of an hour deletes the past hours' counts; later ones leave them", async () => {
  const hour = hourNow();
  worker.sql("DELETE FROM crash_rate; INSERT INTO crash_rate VALUES ('2020-01-01T00', 7), ('2020-01-01T01', 3)");
  const firstStatus = (await worker.post("/v1/crash", crashJson())).status;
  const afterFirst = worker.sql("SELECT hour, n FROM crash_rate ORDER BY hour");
  worker.sql("INSERT INTO crash_rate VALUES ('2020-01-01T00', 7)");
  const secondStatus = (await worker.post("/v1/crash", crashJson())).status;
  const afterSecond = worker.sql("SELECT hour, n FROM crash_rate ORDER BY hour");
  if (hourNow() !== hour) return; // The hour turned mid-test, so the count started again.
  assert.equal(firstStatus, 204);
  assert.deepEqual(afterFirst, [{ hour, n: 1 }], "the first report of the hour");
  assert.equal(secondStatus, 204);
  assert.deepEqual(afterSecond, [{ hour: "2020-01-01T00", n: 7 }, { hour, n: 2 }], "the second");
});

test("a body with spaces and reordered keys is kept in the canonical form", async () => {
  const report = crashJson({ weeksAgo: 2 });
  const reversed = Object.fromEntries(Object.entries(report).reverse());
  assert.equal((await worker.post("/v1/crash", JSON.stringify(reversed, null, 5))).status, 204);
  assert.equal(await worker.object(`reports/${report.report}.json`), JSON.stringify(report));
});

test("anything outside the schema or its 2 KB caps is 400, and is neither counted nor kept", async () => {
  const report = crashJson();
  const edited = (edit) => {
    const one = crashJson({ report: report.report });
    edit(one);
    return one;
  };
  const before = rateTotal();
  const plain = JSON.stringify(report);
  const cases = {
    "an extra field": edited((r) => (r.message = "called `Option::unwrap()` on a `None` value")),
    "an install id": edited((r) => (r.install = "8f9a2c1e-4b7d-4e0a-9c3f-2d6b1a8e5f40")),
    "a path outside the two forms": edited((r) => (r.location.file = "/Users/priya/matter.rs")),
    "an unknown kind": edited((r) => (r.kind = "freeze")),
    "a signal with a location": edited((r) => (r.kind = "signal")),
    "a week 6 weeks old": edited((r) => (r.week = crashJson({ weeksAgo: 6 }).week)),
    "next week": edited((r) => (r.week = crashJson({ weeksAgo: -1 }).week)),
    "a tally": { days: [] },
    "inflates past 2 KB": plain.replace("{", "{" + " ".repeat(2_001 - plain.length)),
  };
  for (const [name, body] of Object.entries(cases)) {
    const response = await worker.post("/v1/crash", body);
    assert.equal(response.status, 400, name);
    assert.equal(await response.text(), "", name);
  }
  const fits = plain.replace("{", "{" + " ".repeat(2_000 - plain.length));
  assert.ok(gzipSync(plain.replace("{", "{" + " ".repeat(2_001 - plain.length))).length < 2_000);
  const big = await worker.send("/v1/crash", { method: "POST", body: randomBytes(2_001), headers: { "content-encoding": "gzip" } });
  assert.equal(big.status, 400, "over 2 KB compressed");
  assert.equal((await worker.post("/v1/crash", plain, { gzip: false })).status, 400, "no gzip");
  assert.deepEqual(rows(report.report), []);
  assert.equal(await worker.object(`reports/${report.report}.json`), null);
  assert.equal(rateTotal(), before, "no refused report was counted");
  assert.equal((await worker.post("/v1/crash", fits)).status, 204, "inflates to exactly 2 KB");
});

test("past 200 reports in one UTC hour is 429: only the count runs, nothing is kept", async () => {
  const hour = hourNow();
  worker.sql(`DELETE FROM crash_rate; INSERT INTO crash_rate VALUES ('2020-01-01T00', 7), ('${hour}', 199)`);
  const last = crashJson();
  const over = crashJson();
  const lastStatus = (await worker.post("/v1/crash", last)).status;
  const overResponse = await worker.post("/v1/crash", over);
  if (hourNow() !== hour) return; // The hour turned mid-test, so the count started again.
  assert.equal(lastStatus, 204, "the 200th");
  assert.equal(overResponse.status, 429, "the 201st");
  assert.equal(await overResponse.text(), "");
  assert.deepEqual(rows(over.report), []);
  assert.equal(await worker.object(`reports/${over.report}.json`), null);
  // The count is the only statement past the cap: no row, no object, no delete of the past hour
  // (and the 200th was not the hour's first report, so it left the past hour too).
  const counts = worker.sql("SELECT hour, n FROM crash_rate ORDER BY hour");
  assert.deepEqual(counts, [{ hour: "2020-01-01T00", n: 7 }, { hour, n: 200 }]);
});

test("a Worker without the bucket bound answers 503 and keeps nothing", async () => {
  const config = readFileSync(join(root, "wrangler.toml"), "utf8");
  assert.ok(config.includes(R2_TABLE));
  writeFileSync(join(root, UNBOUND), config.replace(R2_TABLE, ""));
  const unbound = await startWorker({ config: UNBOUND });
  try {
    const report = crashJson({ report: uuid() });
    const response = await unbound.post("/v1/crash", report);
    assert.equal(response.status, 503);
    assert.equal(await response.text(), "");
    assert.deepEqual(unbound.sql(`SELECT * FROM crash WHERE report = '${report.report}'`), []);
    assert.equal(unbound.sql("SELECT COUNT(*) AS n FROM crash_rate")[0].n, 0);
    // The other routes still work.
    assert.equal((await unbound.post("/v1/crash", { not: "a report" })).status, 400);
  } finally {
    await unbound.stop();
  }
});
