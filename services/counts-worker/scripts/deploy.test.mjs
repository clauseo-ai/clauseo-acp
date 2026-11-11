// node --test scripts/deploy.test.mjs: the deploy job's D1 steps with a stand-in wrangler. Finds
// the database by name, creates it only in the EU and never falls back, writes a checked copy,
// and the `check` action counts one valid id and nothing else.

import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { countQuery, deployConfig, findDatabase, readCount, Refused } from "./deploy.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const committed = readFileSync(join(here, "..", "wrangler.toml"), "utf8");
const ID = "3f2a9c1e-4b7d-4e0a-9c3f-2d6b1a8e5f40";
const INSTALL = "8f9a2c1e-4b7d-4e0a-9c3f-2d6b1a8e5f40";

/** A stand-in wrangler: `lists` are the successive `d1 list` outputs; `create` its status. */
function fakeWrangler(lists, create = { status: 0, stderr: "" }) {
  const calls = [];
  const wrangler = (args) => {
    calls.push(args.join(" "));
    if (args[1] === "list") return { status: 0, stdout: JSON.stringify(lists.shift()), stderr: "" };
    if (args[1] === "create") return { stdout: "", ...create };
    throw new Error(`unexpected ${args.join(" ")}`);
  };
  return { wrangler, calls };
}

test("an existing database is found by name and its id written into a checked copy", () => {
  const { wrangler, calls } = fakeWrangler([[{ name: "other", uuid: "x" }, { name: "clauseo-counts", uuid: ID }]]);
  const copy = deployConfig(wrangler, committed, { create: true });
  assert.ok(copy.includes(`database_id = "${ID}"`));
  assert.deepEqual(calls, ["d1 list --json"]);
});

test("a missing database is created in the EU, and a refusal fails with no fallback", () => {
  const made = fakeWrangler([[], [{ name: "clauseo-counts", uuid: ID }]]);
  deployConfig(made.wrangler, committed, { create: true });
  assert.deepEqual(made.calls, ["d1 list --json", "d1 create clauseo-counts --jurisdiction=eu", "d1 list --json"]);

  const refused = fakeWrangler([[]], { status: 1, stderr: "jurisdiction not allowed" });
  assert.throws(() => deployConfig(refused.wrangler, committed, { create: true }), /--jurisdiction=eu was refused/);
  assert.deepEqual(refused.calls, ["d1 list --json", "d1 create clauseo-counts --jurisdiction=eu"]);

  const checkOnly = fakeWrangler([[]]);
  assert.throws(() => deployConfig(checkOnly.wrangler, committed, { create: false }), Refused);
  assert.deepEqual(checkOnly.calls, ["d1 list --json"]);
});

test("a database outside the EU, two of one name, or a bad id is refused", () => {
  assert.throws(() => findDatabase(JSON.stringify([{ name: "clauseo-counts", uuid: ID, jurisdiction: "fedramp" }])), Refused);
  assert.equal(findDatabase(JSON.stringify([{ name: "clauseo-counts", uuid: ID, jurisdiction: "eu" }])), ID);
  const two = [{ name: "clauseo-counts", uuid: ID }, { name: "clauseo-counts", uuid: ID }];
  assert.throws(() => findDatabase(JSON.stringify(two)), Refused);
  assert.throws(() => findDatabase("not json"), Refused);
  const bad = fakeWrangler([[{ name: "clauseo-counts", uuid: "not-an-id" }]]);
  assert.throws(() => deployConfig(bad.wrangler, committed, { create: true }), /fails check-deploy/);
});

test("the check counts one install or tally id, and only a UUID v4", () => {
  assert.deepEqual(countQuery({ INSTALL }), {
    column: "install",
    sql: `SELECT COUNT(*) AS n FROM report WHERE install = '${INSTALL}'`,
  });
  assert.equal(countQuery({ TALLY: ` ${ID} ` }).sql, `SELECT COUNT(*) AS n FROM tally WHERE tally = '${ID}'`);
  for (const env of [
    {},
    { INSTALL: "", TALLY: "" },
    { INSTALL, TALLY: ID },
    { INSTALL: "x' OR '1'='1" },
    { INSTALL: INSTALL.toUpperCase() },
    { TALLY: "8f9a2c1e-4b7d-1e0a-9c3f-2d6b1a8e5f40" },
    { INSTALL: `${INSTALL}'; DELETE FROM report; --` },
  ]) {
    assert.throws(() => countQuery(env), Refused, JSON.stringify(env));
  }
  assert.equal(readCount(JSON.stringify([{ results: [{ n: 1 }], success: true }])), 1);
  assert.equal(readCount(JSON.stringify([{ results: [{ n: 0 }], success: true }])), 0);
  assert.throws(() => readCount(JSON.stringify([{ results: [{ n: 2 }] }])), Refused);
  assert.throws(() => readCount("garbage"), Refused);
});
