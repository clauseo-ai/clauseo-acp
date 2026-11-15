// node --test scripts/r2.test.mjs: the deploy's R2 step against a stand-in Cloudflare API. It
// finds or creates clauseo-crash in the EU, keeps the 90-day rule beside any other, skips with
// one line on 401 or 403, and prints no id. With the deploy config: the copy binds the bucket
// only when the step says so.

import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { parse } from "smol-toml";
import { BUCKET, RULE, RULE_ID, setUpCrashBucket } from "./r2.mjs";
import { cloudflareApi } from "./cloudflare.mjs";
import { deployConfig, R2_TABLE, Refused } from "./deploy.mjs";

const ACCOUNT = "0123456789abcdef0123456789abcdef";
const here = dirname(fileURLToPath(import.meta.url));
const committed = readFileSync(join(here, "..", "wrangler.toml"), "utf8");

/** A stand-in API over `state` ({ bucket, rules }), recording "METHOD shape [jurisdiction]". */
function fakeApi(state, { deny } = {}) {
  const calls = [];
  const api = async (method, path, body, headers = {}) => {
    calls.push(`${method} ${path.replace(ACCOUNT, "<account>")} ${headers["cf-r2-jurisdiction"] ?? "-"}`);
    if (deny) return { status: deny, json: { success: false } };
    const ok = (result) => ({ status: 200, json: { success: true, result } });
    if (path === "/accounts") return ok([{ id: ACCOUNT }]);
    const base = `/accounts/${ACCOUNT}/r2/buckets`;
    if (method === "GET" && path === `${base}/${BUCKET}`) {
      return state.bucket ? ok({ name: BUCKET, jurisdiction: "eu" }) : { status: 404, json: { success: false } };
    }
    if (method === "POST" && path === base) {
      state.bucket = body.name === BUCKET;
      return ok({ name: body.name, jurisdiction: headers["cf-r2-jurisdiction"] });
    }
    if (method === "GET" && path === `${base}/${BUCKET}/lifecycle`) return ok({ rules: state.rules });
    if (method === "PUT" && path === `${base}/${BUCKET}/lifecycle`) {
      state.rules = body.rules;
      return ok({});
    }
    return { status: 500, json: { success: false } };
  };
  return { api, calls };
}

test("a missing bucket is created in the EU with the 90-day rule; a second run changes nothing", async () => {
  const state = { bucket: false, rules: [] };
  const first = fakeApi(state);
  const result = await setUpCrashBucket(first.api, {});
  assert.equal(result.bound, true);
  assert.deepEqual(first.calls, [
    "GET /accounts -",
    `GET /accounts/<account>/r2/buckets/${BUCKET} eu`,
    "POST /accounts/<account>/r2/buckets eu",
    `GET /accounts/<account>/r2/buckets/${BUCKET}/lifecycle eu`,
    `PUT /accounts/<account>/r2/buckets/${BUCKET}/lifecycle eu`,
  ]);
  assert.deepEqual(state.rules, [RULE]);
  assert.equal(RULE.deleteObjectsTransition.condition.maxAge, 90 * 86_400);
  const again = fakeApi(state);
  await setUpCrashBucket(again.api, { CLOUDFLARE_ACCOUNT_ID: ACCOUNT });
  assert.ok(again.calls.every((call) => call.startsWith("GET")), again.calls.join("\n"));
  assert.ok(!result.lines.join("\n").includes(ACCOUNT));
});

test("other lifecycle rules are kept, and a changed rule of ours is replaced", async () => {
  const other = { id: "abort-uploads", enabled: true, conditions: { prefix: "" } };
  const state = { bucket: true, rules: [other, { ...RULE, deleteObjectsTransition: { condition: { type: "Age", maxAge: 1 } } }] };
  await setUpCrashBucket(fakeApi(state).api, { CLOUDFLARE_ACCOUNT_ID: ACCOUNT });
  assert.deepEqual(state.rules, [other, RULE]);
  assert.equal(state.rules.filter((rule) => rule.id === RULE_ID).length, 1);
});

test("a token without the R2 permission skips with one line, and the copy has no binding", async () => {
  for (const status of [401, 403]) {
    const result = await setUpCrashBucket(fakeApi({}, { deny: status }).api, { CLOUDFLARE_ACCOUNT_ID: ACCOUNT });
    assert.deepEqual(result.bound, false);
    assert.equal(result.lines.length, 1);
    assert.match(result.lines[0], new RegExp(`answered ${status}.*503`));
  }
  await assert.rejects(setUpCrashBucket(fakeApi({}, { deny: 500 }).api, { CLOUDFLARE_ACCOUNT_ID: ACCOUNT }), (error) => {
    assert.ok(!error.message.includes(ACCOUNT), error.message);
    return error instanceof Refused;
  });
});

test("the deploy copy binds the bucket only when the R2 step says so", () => {
  const db = { name: "clauseo-counts", uuid: "3f2a9c1e-4b7d-4e0a-9c3f-2d6b1a8e5f40", jurisdiction: "eu" };
  const wrangler = () => ({ status: 0, stdout: JSON.stringify([db]), stderr: "" });
  assert.ok(committed.includes(R2_TABLE));
  const bound = parse(deployConfig(wrangler, committed, { create: true, bindR2: true }).copy);
  assert.deepEqual(JSON.parse(JSON.stringify(bound.r2_buckets)), [{ binding: "CRASH", bucket_name: BUCKET, jurisdiction: "eu" }]);
  const unbound = parse(deployConfig(wrangler, committed, { create: true }).copy);
  assert.equal(unbound.r2_buckets, undefined);
});

test("the API caller sends the jurisdiction header beside the bearer token", async () => {
  const seen = [];
  const fakeFetch = async (url, init) => {
    seen.push([url, init.headers.Authorization, init.headers["cf-r2-jurisdiction"]]);
    return { status: 200, json: async () => ({ success: true, result: {} }) };
  };
  await cloudflareApi("tok", fakeFetch)("GET", "/x", undefined, { "cf-r2-jurisdiction": "eu" });
  assert.deepEqual(seen, [["https://api.cloudflare.com/client/v4/x", "Bearer tok", "eu"]]);
});
