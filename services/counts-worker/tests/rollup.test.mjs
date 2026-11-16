// pnpm test (tests/rollup.test.mjs): the daily cron's fills under `wrangler dev --test-scheduled`,
// against a fresh local D1, with rows dated from today's UTC clock. It fills only final, whole
// weeks and months, a second run changes nothing, the deletes run first and still run when a
// fill fails, and each run's health lands in cron_status.

import { after, before, test } from "node:test";
import assert from "node:assert/strict";
import { day, mondayAgo, startWorker, uuid, weekOf } from "./harness.mjs";

let worker;
before(async () => {
  worker = await startWorker();
});
after(async () => {
  await worker?.stop();
});

const ids = (count) => Array.from({ length: count }, () => uuid());
const quote = (value) => (value === null ? "NULL" : typeof value === "number" ? String(value) : `'${value}'`);
const values = (rows) => rows.map((row) => `(${row.map(quote).join(", ")})`).join(", ");
const report = (install, dayText, version, cells) =>
  [install, dayText, version, "stable", "macos", 15, "aarch64", "en", JSON.stringify(cells)];
const cron = async () => {
  let response;
  try {
    response = await worker.send("/__scheduled?cron=0+3+*+*+*");
  } catch (error) {
    // The cause names the transport error; wrangler dev's report of a dropped request can land
    // a moment after the fetch fails, so the tail is read a second later.
    const cause = error.cause ? `: ${error.cause.code ?? ""} ${error.cause.message}` : "";
    await new Promise((resolve) => setTimeout(resolve, 1_000));
    throw new Error(`the cron request failed (${error.message}${cause}); wrangler dev said:\n${worker.tail()}`);
  }
  assert.equal(response.status, 200);
};
const list = (installs) => installs.map((id) => `'${id}'`).join(", ");
const snapshot = () => ({
  weeks: worker.sql("SELECT * FROM install_week ORDER BY install, week"),
  rollup: worker.sql("SELECT * FROM rollup ORDER BY month, version, os, event, cell"),
});

// The week of 30 days ago is final (its Sunday at most 24 days back) and whole (its Monday at
// most 36 days back). The week of 2 days ago is not final. The month of 50 days ago is final and
// whole; the month of 5 days ago is not final. The week of 91 days ago is not whole.
const monday = mondayAgo(30);
const wednesday = monday - 2;
const finalWeek = weekOf(day(monday));
const finalMonth = day(50).slice(0, 7);
const weekly = ids(5);
const monthly = ids(5);
const partial = ids(5);
// An earlier row for one install, inside the 396 days install_week keeps, with an older cohort.
const earlierWeek = weekOf(day(200));
const cohort = weekOf(day(300));

test("the cron fills final whole weeks and months, then deletes", async () => {
  const feature = (name) => ({ feature: name, n: 1 });
  const rows = [
    ...weekly.map((install) =>
      report(install, day(monday), "6.0.1", {
        question_sent: [{ agent: "claude", delivery: "now", n: 3 }],
        feature_used: [feature("compare"), feature("versions")],
      }),
    ),
    ...weekly.map((install, i) =>
      report(install, day(wednesday), "6.0.1", {
        question_sent: [{ agent: "chatgpt", delivery: "now", n: 3 }],
        feature_used: [feature("compare")],
        ...(i === 0 ? { limit: [{ agent: "claude", kind: "reached", n: 1 }] } : { limit: [{ agent: "claude", kind: "warned_75", n: 1 }] }),
      }),
    ),
    ...weekly.map((install) => report(install, day(2), "6.0.1", { app_opened: [{ launch: "cold", n: 1 }] })),
    ...monthly.map((install, i) =>
      report(install, day(50), "7.0.1", {
        app_opened: [{ launch: "cold", n: 2 }],
        question_sent: [{ agent: "claude", delivery: "now", n: 1 }],
        ...(i < 4 ? { time_open: [{ bucket: "lt_3h", n: 1 }] } : {}),
      }),
    ),
    ...monthly.map((install) => report(install, day(5), "7.0.1", { app_opened: [{ launch: "cold", n: 1 }] })),
    ...partial.map((install) => report(install, day(91), "8.0.1", { app_opened: [{ launch: "cold", n: 1 }] })),
  ];
  const keptReport = uuid();
  const oldWeek = uuid();
  worker.sql(
    [
      `INSERT INTO report VALUES ${values(rows)}`,
      `INSERT INTO report VALUES ${values([report(keptReport, day(90), "8.0.2", { app_opened: [{ launch: "cold", n: 1 }] })])}`,
      // An earlier cohort for one install, and rows past their time in the summaries.
      `INSERT INTO install_week VALUES ${values([
        [weekly[0], earlierWeek, cohort, 1, 0, 0, 0],
        [oldWeek, weekOf(day(410)), weekOf(day(410)), 1, 0, 0, 0],
      ])}`,
      `INSERT INTO rollup VALUES ${values([["2019-01", "9.0.1", "macos", null, "app_opened", '{"launch":"cold"}', 9, 9]])}`,
    ].join("; "),
  );

  await cron();

  const weeks = worker.sql(`SELECT * FROM install_week WHERE install IN (${list(weekly)}) ORDER BY week`);
  const final = weeks.filter((row) => row.week === finalWeek);
  assert.equal(final.length, 5, "one row per install for the final week");
  for (const row of final) {
    const first = row.install === weekly[0];
    assert.deepEqual(
      { ...row, install: undefined },
      {
        install: undefined,
        week: finalWeek,
        first_week: first ? cohort : finalWeek,
        days_active: 2,
        questions: 6,
        hit_limit: first ? 1 : 0,
        features: 2,
      },
    );
  }
  assert.deepEqual(
    [...new Set(weeks.map((row) => row.week))].sort(),
    [earlierWeek, finalWeek].sort(),
    "the week of 2 days ago is not final, so it has no row",
  );
  assert.equal(worker.sql(`SELECT COUNT(*) AS n FROM install_week WHERE install IN (${list(partial)})`)[0].n, 0, "a week older than 90 days is not whole");

  const rollup = worker.sql("SELECT * FROM rollup WHERE version = '7.0.1' ORDER BY event, cell");
  assert.deepEqual(rollup, [
    { month: finalMonth, version: "7.0.1", os: "macos", agent: null, event: "_report", cell: "{}", count: 5, installs: 5 },
    { month: finalMonth, version: "7.0.1", os: "macos", agent: null, event: "app_opened", cell: '{"launch":"cold"}', count: 10, installs: 5 },
    {
      month: finalMonth,
      version: "7.0.1",
      os: "macos",
      agent: "claude",
      event: "question_sent",
      cell: '{"agent":"claude","delivery":"now"}',
      count: 5,
      installs: 5,
    },
    { month: finalMonth, version: "7.0.1", os: "macos", agent: null, event: "time_open", cell: '{"bucket":"lt_3h"}', count: 4, installs: 4 },
  ]);
  // v_monthly reads rollup with the same rule: the four installs' cell is hidden.
  const monthlyView = worker.sql("SELECT event FROM v_monthly WHERE version = '7.0.1' ORDER BY event").map((row) => row.event);
  assert.deepEqual(monthlyView, ["_report", "app_opened", "question_sent"]);
  assert.equal(worker.sql("SELECT COUNT(*) AS n FROM rollup WHERE version = '8.0.1'")[0].n, 0, "a month with days older than 90 is not whole");

  // The deletes, after the fills: reports past 90 days and install weeks past 396 days go;
  // rollup stays.
  assert.equal(worker.sql(`SELECT COUNT(*) AS n FROM report WHERE install IN (${list(partial)})`)[0].n, 0);
  assert.equal(worker.sql(`SELECT COUNT(*) AS n FROM report WHERE install = '${keptReport}'`)[0].n, 1);
  assert.equal(worker.sql(`SELECT COUNT(*) AS n FROM install_week WHERE install = '${oldWeek}'`)[0].n, 0);
  assert.equal(worker.sql("SELECT COUNT(*) AS n FROM rollup WHERE month = '2019-01'")[0].n, 1);
});

test("a second run changes nothing, even if a report appeared in a filled week", async () => {
  const first = snapshot();
  await cron();
  assert.deepEqual(snapshot(), first);
  // A row the Worker would refuse as too late, written straight into the table: a day of the
  // filled week, with a cell every month that week touches already has.
  const late = report(weekly[1], day(monday - 1), "6.0.1", { feature_used: [{ feature: "compare", n: 5 }] });
  worker.sql(`INSERT INTO report VALUES ${values([late])}`);
  await cron();
  assert.deepEqual(snapshot(), first);
});

test("a failing fill is counted in cron_status, and the deletes still run", async () => {
  const health = () =>
    Object.fromEntries(worker.sql(`SELECT step, ok, failed FROM cron_status WHERE day = '${day(0)}'`).map((row) => [row.step, row]));
  const before = health();
  assert.ok(before.delete.ok >= 1 && before.fill_week.ok >= 1 && before.fill_month.ok >= 1, "earlier runs were recorded");
  assert.deepEqual(Object.keys(worker.sql("SELECT * FROM cron_status LIMIT 1")[0]).sort(), ["day", "failed", "ok", "step"]);
  const old = uuid();
  const fresh = uuid();
  // A trigger makes the week fill fail as it inserts the new install's week; the month fill and
  // the deletes are untouched.
  worker.sql(
    [
      "CREATE TRIGGER no_fill BEFORE INSERT ON install_week BEGIN SELECT RAISE(ABORT, 'test'); END",
      `INSERT INTO report VALUES ${values([
        report(old, day(91), "8.0.3", { app_opened: [{ launch: "cold", n: 1 }] }),
        report(fresh, day(monday), "8.0.3", { app_opened: [{ launch: "cold", n: 1 }] }),
      ])}`,
    ].join("; "),
  );
  try {
    await cron();
  } finally {
    worker.sql("DROP TRIGGER no_fill");
  }
  assert.equal(worker.sql(`SELECT COUNT(*) AS n FROM install_week WHERE install = '${fresh}'`)[0].n, 0);
  const after = health();
  assert.equal(after.fill_week.failed, before.fill_week.failed + 1);
  assert.equal(after.fill_week.ok, before.fill_week.ok);
  assert.equal(after.fill_month.ok, before.fill_month.ok + 1);
  assert.equal(after.delete.ok, before.delete.ok + 1);
  assert.equal(worker.sql(`SELECT COUNT(*) AS n FROM report WHERE install = '${old}'`)[0].n, 0, "the expired report went");
});
