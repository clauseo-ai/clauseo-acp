// pnpm test (tests/access.test.mjs): /dash and /v1/query under `wrangler dev --local`, with Access
// settings passed as --var and a test key pair served through the dev certificate-URL override.
// The JWT checks, the dashboard's headers and charts, every query refusal, the row cap, and the
// dashboard's D1 rows read at 300 installs. (Without the settings both routes are 404:
// worker.test.mjs's Worker has none.)

import { after, before, test } from "node:test";
import assert from "node:assert/strict";
import { createHmac, generateKeyPairSync, sign } from "node:crypto";
import { readFileSync } from "node:fs";
import { createServer } from "node:http";
import { join } from "node:path";
import { gzipSync } from "node:zlib";
import { day as daysAgo, root, startWorker } from "./harness.mjs";

const TEAM = "clauseo-test";
const DASH_AUD = "dash-aud-test";
const QUERY_AUD = "query-aud-test";
const ISSUER = `https://${TEAM}.cloudflareaccess.com`;
const KID = "test-key-1";
const { privateKey, publicKey } = generateKeyPairSync("rsa", { modulusLength: 2048 });
const other = generateKeyPairSync("rsa", { modulusLength: 2048 });

let certs;
let worker;
before(async () => {
  certs = createServer((request, response) => {
    const jwk = { ...publicKey.export({ format: "jwk" }), kid: KID, alg: "RS256", use: "sig" };
    response.setHeader("content-type", "application/json");
    response.end(JSON.stringify({ keys: [jwk] }));
  });
  await new Promise((resolve) => certs.listen(0, "127.0.0.1", resolve));
  worker = await startWorker({
    vars: {
      ACCESS_TEAM: TEAM,
      DASH_AUD,
      QUERY_AUD,
      ACCESS_CERTS_URL_DEV: `http://127.0.0.1:${certs.address().port}/certs`,
    },
  });
});
after(async () => {
  await worker?.stop();
  await new Promise((resolve) => (certs ? certs.close(resolve) : resolve()));
});

const b64 = (value) => Buffer.from(typeof value === "string" ? value : JSON.stringify(value)).toString("base64url");
const now = () => Math.floor(Date.now() / 1000);

/** An Access-like JWT for `aud`, RS256 with the test key unless `edit` says otherwise. */
function jwt(aud, edit = {}) {
  const header = { alg: "RS256", kid: KID, typ: "JWT", ...edit.header };
  const claims = { aud: [aud], iss: ISSUER, exp: now() + 600, iat: now(), nbf: now(), type: "app", ...edit.claims };
  const signed = `${b64(header)}.${b64(claims)}`;
  let signature;
  if (header.alg === "none") signature = "";
  else if (header.alg.startsWith("HS")) signature = createHmac("sha256", "secret").update(signed).digest("base64url");
  else signature = sign("sha256", Buffer.from(signed), edit.key ?? privateKey).toString("base64url");
  return `${signed}.${signature}`;
}

const dash = (token, init = {}) =>
  worker.send("/dash", { ...init, headers: token ? { "cf-access-jwt-assertion": token } : {} });
const query = (sql, token = jwt(QUERY_AUD), { body, headers = {} } = {}) =>
  worker.send("/v1/query", {
    method: "POST",
    body: body ?? JSON.stringify({ sql }),
    headers: { "content-type": "application/json", ...(token ? { "cf-access-jwt-assertion": token } : {}), ...headers },
  });

test("the dashboard needs a valid RS256 token with its own aud", async () => {
  const good = await dash(jwt(DASH_AUD));
  assert.equal(good.status, 200);
  assert.equal(good.headers.get("content-security-policy"), "default-src 'none'; style-src 'unsafe-inline'");
  assert.match(good.headers.get("content-type"), /^text\/html/);
  assert.equal(good.headers.get("cache-control"), "no-store");
  const page = await good.text();
  assert.equal(page.match(/<h2>/g).length, 7, "six charts and the cron's health");
  assert.ok(!/<script|src=|href=|http/i.test(page), "no script and nothing from elsewhere");

  const refused = {
    "no token": undefined,
    "the query route's token": jwt(QUERY_AUD),
    "alg none": jwt(DASH_AUD, { header: { alg: "none" } }),
    "HS256": jwt(DASH_AUD, { header: { alg: "HS256" } }),
    "expired": jwt(DASH_AUD, { claims: { exp: now() - 5 } }),
    "not yet valid": jwt(DASH_AUD, { claims: { nbf: now() + 600 } }),
    "another issuer": jwt(DASH_AUD, { claims: { iss: "https://other.cloudflareaccess.com" } }),
    "an unknown kid": jwt(DASH_AUD, { header: { kid: "nope" } }),
    "another key": jwt(DASH_AUD, { key: other.privateKey }),
    "garbage": "a.b.c",
  };
  for (const [name, token] of Object.entries(refused)) {
    const response = await dash(token);
    assert.equal(response.status, 403, name);
    assert.equal(await response.text(), "", name);
  }
  const post = await worker.send("/dash", { method: "POST", headers: { "cf-access-jwt-assertion": jwt(DASH_AUD) } });
  assert.equal(post.status, 405);
  assert.equal(post.headers.get("allow"), "GET");
});

test("a query over the views returns its rows as JSON", async () => {
  const day = daysAgo(3);
  const rows = Array.from({ length: 5 }, (_, i) =>
    `('0000000${i}-0000-4000-8000-000000000000', '${day}', '9.0.1', 'stable', 'macos', 15, 'aarch64', 'en', '{"app_opened":[{"launch":"cold","n":1}]}')`,
  );
  worker.sql(`INSERT INTO report VALUES ${rows.join(", ")}`);
  // v_daily reads day_cell, which the daily cron fills.
  assert.equal((await worker.send("/__scheduled?cron=0+3+*+*+*")).status, 200);
  const response = await query(`SELECT event, installs FROM v_daily WHERE day = '${day}' ORDER BY event`);
  assert.equal(response.status, 200);
  assert.match(response.headers.get("content-type"), /^application\/json/);
  assert.deepEqual(await response.json(), [
    { event: "_report", installs: 5 },
    { event: "app_opened", installs: 5 },
  ]);
  const capped = await query("WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 5000) SELECT i FROM n");
  assert.equal(capped.status, 200);
  assert.equal((await capped.json()).length, 1000, "capped at 1,000 rows");
});

test("the query route refuses writes, base tables, bad bodies and bad tokens", async () => {
  for (const sql of [
    "INSERT INTO report VALUES (1)",
    "DELETE FROM report",
    "PRAGMA table_info(report)",
    "ATTACH DATABASE 'x' AS y",
    "SELECT 1; SELECT 2",
    "SELECT * FROM report",
    "SELECT * FROM v_daily WHERE version IN (SELECT version FROM report)",
    "WITH c AS (SELECT * FROM report) SELECT * FROM c",
    "SELECT * FROM (WITH report AS (SELECT 1) SELECT 1), report",
    "SELECT * FROM sqlite_master",
    "SELECT * FROM sqlite_schema",
    "SELECT * FROM temp.sqlite_master",
    "SELECT * FROM pragma_table_info('report')",
    "SELECT * FROM main.report",
    'SELECT * FROM "report"',
    "SELECT * FROM [report]",
    "SELECT * FROM `report`",
    "SELECT * FROM REPORT",
    "SELECT * FROM main.v_daily",
    "SELECT * FROM cron_status",
    "SELECT load_extension('x')",
    "SELECT * FROM json_each('[1]')",
    "SELECT nope FROM v_daily",
  ]) {
    const response = await query(sql);
    assert.equal(response.status, 400, sql);
    assert.equal(await response.text(), "", sql);
  }
  assert.equal(worker.sql("SELECT COUNT(*) AS n FROM report")[0].n, 5, "nothing written or deleted");
  assert.equal((await query(undefined, jwt(QUERY_AUD), { body: '{"sql":"SELECT 1","x":1}' })).status, 400, "an extra field");
  assert.equal((await query(undefined, jwt(QUERY_AUD), { body: "not json" })).status, 400);
  const gzip = await query(undefined, jwt(QUERY_AUD), {
    body: gzipSync(JSON.stringify({ sql: "SELECT 1" })),
    headers: { "content-encoding": "gzip" },
  });
  assert.equal(gzip.status, 400, "a gzip body");
  assert.equal((await query("SELECT 1", null)).status, 403, "no token");
  assert.equal((await query("SELECT 1", jwt(DASH_AUD))).status, 403, "the dashboard's token");
  assert.equal((await query("SELECT 1", jwt(QUERY_AUD, { header: { alg: "HS256" } }))).status, 403, "HS256");
  assert.equal((await worker.send("/v1/query", { headers: { "cf-access-jwt-assertion": jwt(QUERY_AUD) } })).status, 405);
});

/** dash.sql's statements, split as src/dash.rs splits them. */
function dashStatements() {
  const code = readFileSync(join(root, "src", "dash.sql"), "utf8")
    .split("\n")
    .filter((line) => !line.trimStart().startsWith("--"))
    .join("\n");
  return code.split(";").map((s) => s.trim()).filter((s) => s !== "");
}

test("one dashboard load reads under 200,000 D1 rows at 300 installs", async () => {
  const series = (name, count) => `${name}(i) AS (SELECT 0 UNION ALL SELECT i + 1 FROM ${name} WHERE i < ${count - 1})`;
  const cells = JSON.stringify({
    app_opened: [{ launch: "cold", n: 2 }],
    time_open: [{ bucket: "lt_3h", n: 1 }],
    question_sent: [{ agent: "claude", delivery: "now", n: 5 }],
    answer_ended: [{ agent: "claude", outcome: "done", took: "lt_10s", n: 3 }],
  });
  worker.sql(
    [
      "DELETE FROM report",
      // 300 installs, each with a report on each of the last 90 days: 27,000 rows.
      `WITH RECURSIVE ${series("a", 300)}, ${series("d", 90)} INSERT INTO report SELECT printf('%08x-0000-4000-8000-000000000001', a.i), date('now', '-' || (d.i + 1) || ' days'), '3.0.' || (a.i % 3), 'stable', 'macos', 15, 'aarch64', 'en', '${cells}' FROM a, d`,
      // 300 installs over 57 weeks, and as many tallies.
      `WITH RECURSIVE ${series("a", 300)}, ${series("w", 57)} INSERT INTO install_week SELECT printf('%08x-0000-4000-8000-000000000001', a.i), strftime('%Y-W%W', 'now', '-' || (w.i * 7 + 20) || ' days'), strftime('%Y-W%W', 'now', '-' || ((a.i % 12) * 7 + 20) || ' days'), 3, 10, 0, 2 FROM a, w`,
      `WITH RECURSIVE ${series("a", 300)}, ${series("w", 57)} INSERT INTO tally SELECT printf('%08x-%04x-4000-8000-000000000002', a.i, w.i), strftime('%Y-W%W', 'now', '-' || (w.i * 7 + 7) || ' days'), '3.0.' || (a.i % 3), 'stable', 'windows', 11, 'x86_64', 'per_user', 0, '{"active_week":[{"n":1}]}' FROM a, w`,
    ].join("; "),
  );
  assert.equal(worker.sql("SELECT COUNT(*) AS n FROM report")[0].n, 27_000);
  // The dashboard reads what the daily cron last filled.
  assert.equal((await worker.send("/__scheduled?cron=0+3+*+*+*")).status, 200);
  const statements = dashStatements();
  assert.equal(statements.length, 6);
  const read = [];
  for (const statement of statements) read.push(await worker.rowsRead(statement));
  assert.ok(read.every(Number.isInteger), "D1 reported rows_read for each statement");
  const total = read.reduce((a, b) => a + b, 0);
  console.log(`dashboard rows read at 300 installs: ${read.join(" + ")} = ${total}`);
  assert.ok(total <= 200_000, `${total} rows read`);
  const page = await dash(jwt(DASH_AUD));
  assert.equal(page.status, 200);
  assert.match(await page.text(), /Active installs/);
});
