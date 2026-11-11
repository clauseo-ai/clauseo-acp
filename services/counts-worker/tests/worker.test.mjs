// pnpm test (tests/worker.test.mjs): the Worker under `wrangler dev --local` with its local D1.
// What it stores, what it refuses with 400, 404 and 405, the canonical form, and the cron.
// The hourly caps are in caps.test.mjs, against a D1 of their own.

import { after, before, test } from "node:test";
import assert from "node:assert/strict";
import { randomBytes } from "node:crypto";
import { gzipSync } from "node:zlib";
import { INSTALL, dayJson, day, startWorker, tallyJson, uuid, week } from "./harness.mjs";

let worker;
before(async () => {
  worker = await startWorker();
});
after(async () => {
  await worker?.stop();
});

const reportRows = (install) => worker.sql(`SELECT * FROM report WHERE install = '${install}' ORDER BY day`);
const rateTotal = () => worker.sql("SELECT COALESCE(SUM(n), 0) AS n FROM rate")[0].n;

test("the crate's example day is stored once, as its fields and canonical cells", async () => {
  const body = { days: [dayJson()] };
  assert.equal((await worker.post("/v1/counts", body)).status, 204);
  const [row] = reportRows(INSTALL);
  assert.deepEqual(
    { ...row, cells: JSON.parse(row.cells) },
    {
      install: INSTALL,
      day: day(1),
      version: "3.0.4",
      channel: "stable",
      os: "macos",
      os_major: 15,
      arch: "aarch64",
      language: "en",
      cells: dayJson().cells,
    },
  );
  assert.ok(!/\s/.test(row.cells), "no whitespace in the stored cells");

  // Sent again, and again with other cells: 204 each time, and the first row stays as it was.
  assert.equal((await worker.post("/v1/counts", body)).status, 204);
  const changed = dayJson();
  changed.cells.app_opened[0].n = 9;
  assert.equal((await worker.post("/v1/counts", { days: [changed] })).status, 204);
  assert.deepEqual(reportRows(INSTALL), [row]);
});

test("seven days in one request are seven rows; Linux leaves os_major NULL", async () => {
  const install = uuid();
  const days = [7, 6, 5, 4, 3, 2, 1].map((daysAgo) => dayJson({ daysAgo, install }));
  days[0].os = "linux";
  delete days[0].os_major;
  assert.equal((await worker.post("/v1/counts", { days })).status, 204);
  const rows = reportRows(install);
  assert.equal(rows.length, 7);
  assert.equal(rows[0].os_major, null);
  assert.equal(rows[0].day, day(7));
});

test("anything outside the schema is 400 with an empty body, and never touches D1", async () => {
  const install = uuid();
  const edited = (edit) => {
    const one = dayJson({ install });
    edit(one);
    return { days: [one] };
  };
  const cases = {
    "an extra field": edited((d) => (d.matter = "Mehra divorce")),
    "an extra field in a cell": edited((d) => (d.cells.app_opened[0].path = "/Users/priya")),
    "an extra field beside days": { days: [dayJson({ install })], note: "x" },
    "an unknown enum value": edited((d) => (d.channel = "nightly")),
    "an unknown error_shown.code": edited((d) => (d.cells.error_shown[0].code = "agent.made_up")),
    "error_shown.code unknown": edited((d) => (d.cells.error_shown[0].code = "unknown")),
    "a day 15 days old": edited((d) => (d.day = day(15))),
    "today": edited((d) => (d.day = day(0))),
    "a day in the future": edited((d) => (d.day = day(-1))),
    "8 days": { days: [8, 7, 6, 5, 4, 3, 2, 1].map((daysAgo) => dayJson({ daysAgo, install })) },
    "no days": { days: [] },
    "a tally": tallyJson(),
  };
  const before = rateTotal();
  for (const [name, body] of Object.entries(cases)) {
    const response = await worker.post("/v1/counts", body);
    assert.equal(response.status, 400, name);
    assert.equal(await response.text(), "", name);
  }
  assert.equal((await worker.post("/v1/counts", "not json")).status, 400);
  assert.deepEqual(reportRows(install), []);
  assert.equal(rateTotal(), before, "no refused request bumped the hourly count");
});

test("the body must be gzip, at most 16 KB compressed and 64 KB inflated", async () => {
  const install = uuid();
  const plain = JSON.stringify({ days: [dayJson({ install })] });
  const before = rateTotal();
  assert.equal((await worker.post("/v1/counts", plain, { gzip: false })).status, 400, "no gzip");
  const unzipped = await worker.send("/v1/counts", { method: "POST", body: plain, headers: { "content-encoding": "gzip" } });
  assert.equal(unzipped.status, 400, "says gzip, is not");
  const brotli = await worker.send("/v1/counts", { method: "POST", body: gzipSync(plain), headers: { "content-encoding": "br" } });
  assert.equal(brotli.status, 400, "another encoding");
  const big = await worker.send("/v1/counts", { method: "POST", body: randomBytes(17_000), headers: { "content-encoding": "gzip" } });
  assert.equal(big.status, 400, "a 17 KB body");
  // A valid batch padded with spaces to just past 64 KB: a few hundred bytes of gzip.
  const padded = plain.replace("{", "{" + " ".repeat(64_001 - plain.length));
  assert.ok(gzipSync(padded).length < 16_000);
  assert.equal((await worker.post("/v1/counts", padded)).status, 400, "inflates past 64 KB");
  const fits = plain.replace("{", "{" + " ".repeat(64_000 - plain.length));
  assert.equal((await worker.post("/v1/counts", fits)).status, 204, "inflates to exactly 64 KB");
  assert.equal(reportRows(install).length, 1);
  assert.equal(rateTotal(), before + 1);
});

test("a body with spaces and reordered keys is stored in the canonical form", async () => {
  const tidy = uuid();
  const messy = uuid();
  assert.equal((await worker.post("/v1/counts", { days: [dayJson({ install: tidy })] })).status, 204);
  const reorder = (value) =>
    Array.isArray(value)
      ? value.map(reorder)
      : value && typeof value === "object"
        ? Object.fromEntries(Object.entries(value).reverse().map(([k, v]) => [k, reorder(v)]))
        : value;
  const text = JSON.stringify(reorder({ days: [dayJson({ install: messy })] }), null, 7);
  assert.equal((await worker.post("/v1/counts", text)).status, 204);
  const [a] = reportRows(tidy);
  const [b] = reportRows(messy);
  assert.equal(b.cells, a.cells);
  assert.deepEqual({ ...b, install: tidy }, a);
});

test("a tally is stored once per tally id, and refused outside its rules", async () => {
  const id = uuid();
  const tallyRows = () => worker.sql(`SELECT * FROM tally WHERE tally = '${id}'`);
  assert.equal((await worker.post("/v1/tally", tallyJson({ tally: id }))).status, 204);
  assert.equal((await worker.post("/v1/tally", tallyJson({ tally: id, weeksAgo: 2 }))).status, 204);
  const rows = tallyRows();
  assert.equal(rows.length, 1);
  assert.deepEqual(
    { ...rows[0], cells: JSON.parse(rows[0].cells) },
    {
      tally: id,
      week: week(1),
      version: "3.0.4",
      channel: "stable",
      os: "windows",
      os_major: 11,
      arch: "x86_64",
      install_type: "per_machine",
      managed: 1,
      cells: tallyJson().cells,
    },
  );

  const other = uuid();
  const withInstall = { ...tallyJson({ tally: other }), install: INSTALL };
  const plain = JSON.stringify(tallyJson({ tally: other }));
  const cases = {
    "a week 5 weeks old": tallyJson({ tally: other, weeksAgo: 5 }),
    "this week": tallyJson({ tally: other, weeksAgo: 0 }),
    "an install field": withInstall,
    "a day batch": { days: [dayJson()] },
    "inflates past 16 KB": plain.replace("{", "{" + " ".repeat(16_001 - plain.length)),
  };
  for (const [name, body] of Object.entries(cases)) {
    assert.equal((await worker.post("/v1/tally", body)).status, 400, name);
  }
  const big = await worker.send("/v1/tally", { method: "POST", body: randomBytes(4_001), headers: { "content-encoding": "gzip" } });
  assert.equal(big.status, 400, "over 4 KB compressed");
  assert.equal((await worker.post("/v1/tally", tallyJson({ tally: other, weeksAgo: 4 }))).status, 204, "4 weeks old");
  assert.equal(worker.sql(`SELECT COUNT(*) AS n FROM tally WHERE tally = '${other}'`)[0].n, 1);
});

test("any other path is 404, another method on the two paths 405", async () => {
  for (const path of ["/", "/v1/crash", "/v1/query", "/v1/releases", "/dash", "/v1/counts/", "/v1/Counts"]) {
    assert.equal((await worker.post(path, { days: [dayJson()] })).status, 404, path);
    assert.equal((await worker.send(path)).status, 404, `GET ${path}`);
  }
  for (const path of ["/v1/counts", "/v1/tally"]) {
    for (const method of ["GET", "PUT", "DELETE", "PATCH"]) {
      const response = await worker.send(path, { method });
      assert.equal(response.status, 405, `${method} ${path}`);
      assert.equal(response.headers.get("allow"), "POST");
      assert.equal(await response.text(), "");
    }
  }
});

test("the weekly cron deletes reports after 90 days, tallies after 396, counts after 24 hours", async () => {
  const hour = Math.floor(Date.now() / 3_600_000);
  const old = uuid();
  const kept = uuid();
  const insert = (install, dayText) =>
    `INSERT INTO report VALUES ('${install}', '${dayText}', '3.0.4', 'stable', 'macos', 15, 'aarch64', 'en', '{}')`;
  const tally = (id, weekText) =>
    `INSERT INTO tally VALUES ('${id}', '${weekText}', '3.0.4', 'stable', 'linux', NULL, 'x86_64', 'per_user', 0, '{}')`;
  const oldTally = uuid();
  const keptTally = uuid();
  worker.sql(
    [
      insert(old, day(91)),
      insert(kept, day(90)),
      tally(oldTally, week(58)),
      tally(keptTally, week(55)),
      `INSERT INTO rate VALUES ('counts', ${hour - 25}, 3), ('tally', ${hour - 23}, 4)`,
    ].join("; "),
  );
  const response = await worker.send("/__scheduled?cron=0+3+*+*+1");
  assert.equal(response.status, 200);
  const ids = (table, column, list) =>
    worker.sql(`SELECT ${column} AS id FROM ${table} WHERE ${column} IN (${list.map((id) => `'${id}'`).join(", ")})`).map((row) => row.id);
  assert.deepEqual(ids("report", "install", [old, kept]), [kept]);
  assert.deepEqual(ids("tally", "tally", [oldTally, keptTally]), [keptTally]);
  assert.deepEqual(worker.sql(`SELECT hour FROM rate WHERE hour < ${hour - 20}`), [{ hour: hour - 23 }]);
});
