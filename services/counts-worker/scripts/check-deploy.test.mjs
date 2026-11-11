// node --test scripts/check-deploy.test.mjs: the guard passes the committed wrangler.toml and
// refuses each departure the Worker's privacy rests on.

import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { problems, PLACEHOLDER_ID } from "./check-deploy.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const committed = readFileSync(join(here, "..", "wrangler.toml"), "utf8");
const REAL_ID = "3f2a9c1e-4b7d-4e0a-9c3f-2d6b1a8e5f40";

/** The committed file with `from` replaced by `to`, which must be there. */
function edited(from, to, text = committed) {
  assert.ok(text.includes(from), `the file holds ${from}`);
  return text.replace(from, to);
}

test("the committed file passes, and only with the placeholder id", () => {
  assert.deepEqual(problems(committed), []);
  assert.equal(problems(committed, { deployed: true }).length, 1);
  const deployCopy = edited(PLACEHOLDER_ID, REAL_ID);
  assert.deepEqual(problems(deployCopy, { deployed: true }), []);
  assert.equal(problems(deployCopy).length, 1);
  assert.equal(problems(edited(PLACEHOLDER_ID, "not-an-id"), { deployed: true }).length, 1);
});

test("logs, traces and observability must each be off", () => {
  for (const [from, to] of [
    ["[observability]\nenabled = false", "[observability]\nenabled = true"],
    ["[observability]\nenabled = false", "[observability]"],
    ["invocation_logs = false", "invocation_logs = true"],
    ["invocation_logs = false\n", ""],
    ["[observability.logs]\nenabled = false", "[observability.logs]\nenabled = true"],
    ["[observability.traces]\nenabled = false", "[observability.traces]\nenabled = true"],
    ["[observability.logs]\nenabled = false", "[observability.logs]\nenabled = false\nhead_sampling_rate = 1"],
    ["[observability]\nenabled = false", "[observability]\nenabled = false\nhead_sampling_rate = 1"],
  ]) {
    assert.notDeepEqual(problems(edited(from, to)), [], `${from} -> ${to}`);
  }
});

test("no workers.dev, preview URL, Tail Worker, Logpush, var or extra environment", () => {
  for (const [from, to] of [
    ["workers_dev = false", "workers_dev = true"],
    ["workers_dev = false\n", ""],
    ["preview_urls = false", "preview_urls = true"],
    ["preview_urls = false\n", ""],
    ["workers_dev = false", 'workers_dev = false\ntail_consumers = [{ service = "tail" }]'],
    ["workers_dev = false", 'workers_dev = false\nstreaming_tail_consumers = [{ service = "tail" }]'],
    ["workers_dev = false", "workers_dev = false\nlogpush = true"],
    ["workers_dev = false", "workers_dev = false\nlogpush = false"],
    ["[build]", '[vars]\nHOURLY_CAP = "9999"\n\n[build]'],
    ["[build]", '[env.staging]\nworkers_dev = true\n\n[build]'],
    ["[build]", '[[r2_buckets]]\nbinding = "CRASH"\nbucket_name = "x"\n\n[build]'],
  ]) {
    assert.notDeepEqual(problems(edited(from, to)), [], `${from} -> ${to}`);
  }
});

test("the route is the custom domain alone, and the database the one binding", () => {
  for (const [from, to] of [
    ["custom_domain = true", "custom_domain = false"],
    ['pattern = "t.clauseo.chat"', 'pattern = "t.clauseo.chat/*"'],
    [
      'routes = [{ pattern = "t.clauseo.chat", custom_domain = true }]',
      'routes = [{ pattern = "t.clauseo.chat", custom_domain = true }, { pattern = "x.clauseo.chat", custom_domain = true }]',
    ],
    ['binding = "DB"', 'binding = "OTHER"'],
    ['database_name = "clauseo-counts"', 'database_name = "other"'],
    ['migrations_dir = "migrations"', 'migrations_dir = "migrations"\npreview_database_id = "x"'],
    ['crons = ["0 3 * * 1"]', 'crons = ["0 3 * * 1", "* * * * *"]'],
    ['command = "worker-build --release"', 'command = "worker-build --dev"'],
  ]) {
    assert.notDeepEqual(problems(edited(from, to)), [], `${from} -> ${to}`);
  }
  const twoDatabases = committed + '\n[[d1_databases]]\nbinding = "B"\ndatabase_name = "b"\ndatabase_id = "x"\n';
  assert.notDeepEqual(problems(twoDatabases), []);
  assert.deepEqual(problems("not [toml").length, 1);
});
