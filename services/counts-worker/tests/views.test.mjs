// pnpm test (tests/views.test.mjs): the v_* views of migrations 0002 and 0003, against rows
// written straight into a fresh local D1. v_daily and v_active read the cron's summaries, so
// their test runs the cron first. In every view a group of 4 is hidden and a group of 5 shown,
// and no view has an install or tally id column.

import { after, before, test } from "node:test";
import assert from "node:assert/strict";
import { day, startWorker, uuid } from "./harness.mjs";

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

/** One `report` row's values: version 9.0.1 on macOS 15 unless said. */
const report = (install, day, cells, { version = "9.0.1", os = "macos" } = {}) =>
  [install, day, version, "stable", os, 15, "aarch64", "en", JSON.stringify(cells)];

const QUESTION = { question_sent: [{ agent: "claude", delivery: "now", n: 2 }] };
const BOTH = { ...QUESTION, feature_used: [{ feature: "compare", n: 1 }] };

/** The view's columns, from PRAGMA table_info. */
const columns = (view) => worker.sql(`PRAGMA table_info(${view})`).map((column) => column.name);

test("every view's columns: no install, tally or report id", () => {
  const views = worker.sql("SELECT name FROM sqlite_master WHERE type = 'view' ORDER BY name").map((row) => row.name);
  const expected = {
    v_active: ["period", "start", "installs"],
    v_cohort_week: ["first_week", "week", "installs", "days_active", "questions", "hit_limit", "features"],
    v_crash: ["week", "version", "os", "arch", "kind", "area", "signature", "reports"],
    v_daily: ["day", "version", "os", "agent", "event", "cell", "count", "installs"],
    v_monthly: ["month", "version", "os", "agent", "event", "cell", "count", "installs"],
    v_paths: ["weeks_active", "questions", "hit_limit", "features", "installs"],
    v_tally: ["week", "version", "os", "event", "cell", "count", "tallies"],
  };
  assert.deepEqual(views, Object.keys(expected));
  for (const view of views) {
    const names = columns(view);
    assert.deepEqual(names, expected[view], view);
    for (const id of ["install", "tally", "report"]) assert.ok(!names.includes(id), `${view} has ${id}`);
  }
});

test("v_daily and v_active: 4 installs hidden, 5 shown, from the cron's summaries", async () => {
  const five = ids(5);
  const four = ids(4);
  const a = ids(5);
  const b = ids(5);
  const c = ids(5);
  // Days still open (under 15 days old), so the cron fills no week or month from them and the
  // later tests' views see only their own rows. Far-future days are never deleted or filled
  // into install_week, and give ISO weeks across a turn of the year.
  const rows = [
    ...five.map((install, i) => report(install, day(3), i < 4 ? BOTH : QUESTION)),
    ...five.slice(0, 4).map((install) => report(install, day(4), QUESTION)),
    ...four.map((install) => report(install, day(5), QUESTION)),
    ...a.map((install) => report(install, "2995-12-31", QUESTION)),
    ...a.map((install) => report(install, "2996-01-03", QUESTION)),
    ...b.map((install) => report(install, "2996-01-04", QUESTION)),
    ...c.map((install) => report(install, "2994-12-31", QUESTION)),
  ];
  worker.sql(`INSERT INTO report VALUES ${values(rows)}`);
  assert.deepEqual(worker.sql("SELECT * FROM v_daily"), [], "nothing before the cron fills day_cell");
  assert.equal((await worker.send("/__scheduled?cron=0+3+*+*+*")).status, 200);

  const daily = worker.sql(`SELECT * FROM v_daily WHERE day IN ('${day(3)}', '${day(4)}', '${day(5)}') ORDER BY event`);
  assert.deepEqual(daily, [
    { day: day(3), version: "9.0.1", os: "macos", agent: null, event: "_report", cell: "{}", count: 5, installs: 5 },
    {
      day: day(3),
      version: "9.0.1",
      os: "macos",
      agent: "claude",
      event: "question_sent",
      cell: '{"agent":"claude","delivery":"now"}',
      count: 10,
      installs: 5,
    },
  ]);
  // The four installs' feature cell is in day_cell, hidden by the view.
  assert.equal(worker.sql(`SELECT installs FROM day_cell WHERE day = '${day(3)}' AND event = 'feature_used'`)[0].installs, 4);

  const days = worker.sql(`SELECT * FROM v_active WHERE period = 'day' AND start IN ('${day(3)}', '${day(4)}', '${day(5)}')`);
  assert.deepEqual(days, [{ period: "day", start: day(3), installs: 5 }]);
  const future = worker.sql("SELECT * FROM v_active WHERE start >= '2994' ORDER BY period, start");
  assert.deepEqual(future, [
    { period: "day", start: "2994-12-31", installs: 5 },
    { period: "day", start: "2995-12-31", installs: 5 },
    { period: "day", start: "2996-01-03", installs: 5 },
    { period: "day", start: "2996-01-04", installs: 5 },
    { period: "month", start: "2994-12", installs: 5 },
    { period: "month", start: "2995-12", installs: 5 },
    { period: "month", start: "2996-01", installs: 10 },
    { period: "week", start: "2995-W01", installs: 5 },
    { period: "week", start: "2995-W53", installs: 5 },
    { period: "week", start: "2996-W01", installs: 5 },
  ]);
});

test("v_monthly: a rollup row of 4 installs hidden, 5 shown", () => {
  worker.sql(
    `INSERT INTO rollup VALUES ${values([
      ["2019-01", "9.0.1", "macos", null, "app_opened", '{"launch":"cold"}', 40, 4],
      ["2019-01", "9.0.1", "macos", null, "app_opened", '{"launch":"after_update"}', 50, 5],
    ])}`,
  );
  assert.deepEqual(worker.sql("SELECT * FROM v_monthly"), [
    { month: "2019-01", version: "9.0.1", os: "macos", agent: null, event: "app_opened", cell: '{"launch":"after_update"}', count: 50, installs: 5 },
  ]);
});

test("v_cohort_week and v_paths: 4 installs hidden, 5 shown", () => {
  const five = ids(5);
  const four = ids(4);
  worker.sql(
    `INSERT INTO install_week VALUES ${values([
      ...five.map((install, i) => [install, "2019-W02", "2019-W01", 3, 12, i === 0 ? 1 : 0, 3]),
      ...four.map((install) => [install, "2019-W03", "2019-W03", 1, 0, 0, 0]),
    ])}`,
  );
  assert.deepEqual(worker.sql("SELECT * FROM v_cohort_week"), [
    { first_week: "2019-W01", week: "2019-W02", installs: 5, days_active: 15, questions: 60, hit_limit: 1, features: 15 },
  ]);
  // The five fall in two groups once one has reached a limit: so add a sixth like the four.
  worker.sql(`INSERT INTO install_week VALUES ${values([[uuid(), "2019-W02", "2019-W01", 3, 12, 0, 3]])}`);
  assert.deepEqual(worker.sql("SELECT * FROM v_paths"), [{ weeks_active: 1, questions: 10, hit_limit: 0, features: 2, installs: 5 }]);
});

test("v_tally: 4 tallies hidden, 5 shown", () => {
  const crash = { process: "webview", kind: "renderer_gone", after_launch: false, n: 1 };
  const tally = (week, cells) => [uuid(), week, "9.0.1", "stable", "windows", 11, "x86_64", "per_machine", 1, JSON.stringify(cells)];
  const rows = [
    ...[0, 1, 2, 3, 4].map((i) => tally("2019-W05", i < 4 ? { crash: [crash], active_week: [{ n: 1 }] } : { active_week: [{ n: 1 }] })),
    ...[0, 1, 2, 3].map(() => tally("2019-W06", { crash: [crash], active_week: [{ n: 1 }] })),
  ];
  worker.sql(`INSERT INTO tally VALUES ${values(rows)}`);
  const shown = worker.sql("SELECT event, cell, count, tallies FROM v_tally ORDER BY event");
  assert.deepEqual(shown, [
    { event: "_fleet", cell: '{"install_type":"per_machine","managed":1}', count: 5, tallies: 5 },
    { event: "_tally", cell: "{}", count: 5, tallies: 5 },
    { event: "active_week", cell: "{}", count: 5, tallies: 5 },
  ]);
  assert.ok(worker.sql("SELECT * FROM v_tally").every((row) => row.week === "2019-W05" && row.version === "9.0.1"));
});

test("v_crash: 4 reports hidden, 5 shown, per signature and per kind and area", () => {
  const crash = (week, signature) => [uuid(), week, "9.0.1", "windows", "x86_64", "panic", "store", signature];
  const rows = [
    ...[0, 1, 2, 3, 4].map(() => crash("2019-W05", "crates/store/src/writer.rs:212")),
    ...[0, 1, 2, 3].map(() => crash("2019-W05", "crates/store/src/log.rs:40")),
    ...[0, 1, 2, 3].map(() => crash("2019-W06", "crates/store/src/writer.rs:212")),
  ];
  worker.sql(`INSERT INTO crash VALUES ${values(rows)}`);
  const shown = worker.sql("SELECT week, signature, reports FROM v_crash ORDER BY week, signature");
  assert.deepEqual(shown, [
    { week: "2019-W05", signature: "_all", reports: 9 },
    { week: "2019-W05", signature: "crates/store/src/writer.rs:212", reports: 5 },
  ]);
});
