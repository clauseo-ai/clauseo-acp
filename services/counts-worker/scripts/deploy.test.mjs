// node --test scripts/deploy.test.mjs: the deploy job's wrangler steps with a stand-in wrangler.
// Finds the database by name and only in the EU, creates it only in the EU and never falls back,
// writes a checked copy without [build], counts and deletes only the canary, and masks every
// id, email and account name in what it prints.

import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { parse } from "smol-toml";
import {
  CANARY_COUNT,
  CANARY_DELETE,
  CANARY_INSTALL,
  CANARY_TALLY,
  deployConfig,
  findDatabase,
  readCanary,
  redact,
  Refused,
  wranglerEnv,
} from "./deploy.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const committed = readFileSync(join(here, "..", "wrangler.toml"), "utf8");
const ID = "3f2a9c1e-4b7d-4e0a-9c3f-2d6b1a8e5f40";
const db = (fields = {}) => ({ name: "clauseo-counts", uuid: ID, jurisdiction: "eu", ...fields });

/** A stand-in wrangler: `lists` are the successive `d1 list` outputs; `create` its result. */
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

test("an existing EU database is found by name, and the copy has its id and no [build]", () => {
  const { wrangler, calls } = fakeWrangler([[{ name: "other", uuid: "x" }, db()]]);
  const { id, copy } = deployConfig(wrangler, committed, { create: true });
  assert.equal(id, ID);
  const config = parse(copy);
  assert.equal(config.d1_databases[0].database_id, ID);
  assert.equal(config.build, undefined);
  assert.equal(config.main, "build/worker/shim.mjs");
  assert.deepEqual(calls, ["d1 list --json"]);
});

test("a missing database is created in the EU, and a refusal fails with no fallback", () => {
  const made = fakeWrangler([[], [db()]]);
  deployConfig(made.wrangler, committed, { create: true });
  assert.deepEqual(made.calls, ["d1 list --json", "d1 create clauseo-counts --jurisdiction=eu", "d1 list --json"]);

  const refused = fakeWrangler([[]], { status: 1, stderr: "jurisdiction not allowed for rohan@clauseo.chat" });
  assert.throws(
    () => deployConfig(refused.wrangler, committed, { create: true }),
    (error) => error instanceof Refused && /--jurisdiction=eu was refused/.test(error.message) && !error.message.includes("rohan@"),
  );
  assert.deepEqual(refused.calls, ["d1 list --json", "d1 create clauseo-counts --jurisdiction=eu"]);

  const checkOnly = fakeWrangler([[]]);
  assert.throws(() => deployConfig(checkOnly.wrangler, committed, { create: false }), Refused);
  assert.deepEqual(checkOnly.calls, ["d1 list --json"]);
});

test("a database outside the EU or with no jurisdiction, two of one name, or a bad id is refused", () => {
  for (const jurisdiction of ["fedramp", "us", undefined, null, "EU"]) {
    assert.throws(() => findDatabase(JSON.stringify([db({ jurisdiction })])), Refused, String(jurisdiction));
  }
  assert.equal(findDatabase(JSON.stringify([db()])), ID);
  assert.throws(() => findDatabase(JSON.stringify([db(), db()])), Refused);
  assert.throws(() => findDatabase("not json"), Refused);
  const bad = fakeWrangler([[db({ uuid: "not-an-id" })]]);
  assert.throws(() => deployConfig(bad.wrangler, committed, { create: true }), /fails check-deploy/);
});

test("the canary counts and deletes its two fixed ids and nothing else", () => {
  assert.match(CANARY_INSTALL, /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/);
  assert.match(CANARY_TALLY, /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/);
  for (const sql of [CANARY_COUNT, CANARY_DELETE]) {
    const ids = sql.match(/'[^']*'/g);
    assert.deepEqual(ids, [`'${CANARY_INSTALL}'`, `'${CANARY_TALLY}'`], sql);
  }
  assert.equal(CANARY_DELETE.match(/DELETE FROM report WHERE install = /g).length, 1);
  assert.deepEqual(readCanary(JSON.stringify([{ results: [{ report: 3, tally: 1 }], success: true }])), { report: 3, tally: 1 });
  assert.deepEqual(readCanary(JSON.stringify([{ results: [{ report: 0, tally: 0 }] }])), { report: 0, tally: 0 });
  assert.throws(() => readCanary(JSON.stringify([{ results: [{ report: "x", tally: 0 }] }])), Refused);
  assert.throws(() => readCanary("garbage"), Refused);
});

test("the redactor masks ids, emails and account names in wrangler's output", () => {
  const accounts = [
    "More than one account available but unable to select one in non-interactive mode.",
    "Available accounts are (`<name>`: `<account_id>`):",
    "  `Rohan Shiralkar's Account`: `0123456789abcdef0123456789ABCDEF`",
    "  `rohan@clauseo.chat's Account`: `fedcba9876543210fedcba9876543210`",
  ].join("\n");
  const masked = redact(accounts);
  assert.ok(!/Rohan|rohan|clauseo\.chat|0123456789|fedcba/.test(masked), masked);
  assert.ok(masked.includes("More than one account available"));

  const deploy = [
    "Uploaded clauseo-counts (3.21 sec)",
    "Deployed clauseo-counts triggers (1.05 sec)",
    "  t.clauseo.chat (custom domain)",
    `Current Version ID: ${ID}`,
    `https://dash.cloudflare.com/0123456789abcdef0123456789abcdef/workers/d1/databases/${ID}`,
    "Logged in as someone@example.org",
  ].join("\n");
  const shown = redact(deploy);
  assert.ok(!shown.includes(ID) && !shown.includes("0123456789abcdef") && !shown.includes("someone@"), shown);
  assert.ok(shown.includes("t.clauseo.chat (custom domain)"), shown);
  assert.equal(redact("abc 0123456789abcdef0123456789abcdef0 def"), "abc 0123456789abcdef0123456789abcdef0 def");
});

test("an empty account id is left out, so wrangler picks the one account", () => {
  assert.equal("CLOUDFLARE_ACCOUNT_ID" in wranglerEnv({ CLOUDFLARE_ACCOUNT_ID: "", A: "1" }), false);
  assert.equal(wranglerEnv({ CLOUDFLARE_ACCOUNT_ID: "abc" }).CLOUDFLARE_ACCOUNT_ID, "abc");
  assert.equal(wranglerEnv({}).WRANGLER_SEND_METRICS, "false");
});
