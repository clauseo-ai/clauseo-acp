// pnpm test (tests/views.test.mjs): the v_* views of migrations/0002_rollup.sql, against rows
// written straight into a fresh local D1 (dated in the past, as the views do not look at the
// clock). In every view a group of 4 is hidden and a group of 5 shown, and no view has an
// install or tally id column.

import { after, before, test } from "node:test";
import assert from "node:assert/strict";
import { startWorker, uuid } from "./harness.mjs";

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

test("v_daily and v_active: 4 installs hidden, 5 shown", () => {
  const five = ids(5);
  const four = ids(4);
  const a = ids(5);
  const b = ids(5);
  const c = ids(5);
  const rows = [
    // 2020-03-02: five send a question, four of them a feature too. 2020-03-03: four send.
    ...five.map((install, i) => report(install, "2020-03-02", i < 4 ? BOTH : QUESTION)),
    ...five.slice(0, 4).map((install) => report(install, "2020-03-03", QUESTION)),
    // A week and month of four alone.
    ...four.map((install) => report(install, "2020-06-10", QUESTION)),
    // ISO weeks across the turn of the year.
    ...a.map((install) => report(install, "2026-12-31", QUESTION)),
    ...a.map((install) => report(install, "2027-01-03", QUESTION)),
    ...b.map((install) => report(install, "2027-01-04", QUESTION)),
    ...c.map((install) => report(install, "2024-12-30", QUESTION)),
  ];
  worker.sql(`INSERT INTO report VALUES ${values(rows)}`);

  const daily = worker.sql("SELECT * FROM v_daily WHERE day IN ('2020-03-02', '2020-03-03', '2020-06-10') ORDER BY event");
  assert.deepEqual(daily, [
    { day: "2020-03-02", version: "9.0.1", os: "macos", agent: null, event: "_report", cell: "{}", count: 5, installs: 5 },
    {
      day: "2020-03-02",
      version: "9.0.1",
      os: "macos",
      agent: "claude",
      event: "question_sent",
      cell: '{"agent":"claude","delivery":"now"}',
      count: 10,
      installs: 5,
    },
  ]);

  const active = worker.sql("SELECT * FROM v_active ORDER BY period, start");
  assert.deepEqual(active, [
    { period: "day", start: "2020-03-02", installs: 5 },
    { period: "day", start: "2024-12-30", installs: 5 },
    { period: "day", start: "2026-12-31", installs: 5 },
    { period: "day", start: "2027-01-03", installs: 5 },
    { period: "day", start: "2027-01-04", installs: 5 },
    { period: "month", start: "2020-03", installs: 5 },
    { period: "month", start: "2024-12", installs: 5 },
    { period: "month", start: "2026-12", installs: 5 },
    { period: "month", start: "2027-01", installs: 10 },
    { period: "week", start: "2020-W10", installs: 5 },
    { period: "week", start: "2025-W01", installs: 5 },
    { period: "week", start: "2026-W53", installs: 5 },
    { period: "week", start: "2027-W01", installs: 5 },
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
