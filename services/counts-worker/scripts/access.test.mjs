// node --test scripts/access.test.mjs: the deploy's Access step against a stand-in Cloudflare
// API. It finds or creates the two applications, keeps one policy each, reads their AUD tags,
// skips with one line when ACCESS_TEAM is unset or the token lacks the scopes, and never puts an
// id, AUD, team or email in what it prints.

import { test } from "node:test";
import assert from "node:assert/strict";
import { APPS, cloudflareApi, setUpAccess, SERVICE_TOKEN, varArgs } from "./access.mjs";
import { Refused, redact } from "./deploy.mjs";

const ACCOUNT = "0123456789abcdef0123456789abcdef";
const AUD_DASH = "a".repeat(64);
const AUD_QUERY = "b".repeat(64);
const EMAIL = "owner@example.org";
const TEAM = "clauseo-team";
const TOKEN_ID = "the-agent-token-test-id";

/**
 * A stand-in API over `state` ({ apps, policies: { appId: [...] }, tokens }), recording each call
 * as "METHOD path-shape". `deny` answers every call with that status.
 */
function fakeApi(state, { deny } = {}) {
  const calls = [];
  let next = 1;
  const api = async (method, path, body) => {
    const shape = path.replace(ACCOUNT, "<account>").replace(/\/app-\d+/, "/<app>").replace(/\/pol-\d+/, "/<policy>");
    calls.push(`${method} ${shape}`);
    if (deny) return { status: deny, json: { success: false } };
    const ok = (result) => ({ status: 200, json: { success: true, result } });
    if (method === "GET" && path === "/accounts") return ok([{ id: ACCOUNT, name: "Someone's Account" }]);
    const base = `/accounts/${ACCOUNT}/access`;
    if (method === "GET" && path.startsWith(`${base}/apps?`)) return ok(state.apps);
    if (method === "GET" && path.startsWith(`${base}/service_tokens?`)) return ok(state.tokens);
    if (method === "POST" && path === `${base}/apps`) {
      const app = { id: `app-${next++}`, aud: body.name.endsWith("dash") ? AUD_DASH : AUD_QUERY, ...body };
      state.apps.push(app);
      return ok(app);
    }
    const policyPath = path.match(/\/apps\/(app-\d+)\/policies(?:\/(pol-\d+))?$/);
    if (policyPath) {
      const [, appId, policyId] = policyPath;
      state.policies[appId] ??= [];
      if (method === "GET") return ok(state.policies[appId]);
      if (method === "POST") {
        const policy = { id: `pol-${next++}`, ...body };
        state.policies[appId].push(policy);
        return ok(policy);
      }
      if (method === "PUT") {
        const list = state.policies[appId];
        list[list.findIndex((p) => p.id === policyId)] = { id: policyId, ...body };
        return ok(body);
      }
    }
    return { status: 404, json: { success: false } };
  };
  return { api, calls };
}

const env = (extra = {}) => ({ ACCESS_TEAM: TEAM, ACCESS_DASH_EMAIL: EMAIL, ...extra });
const printed = (result) => result.lines.join("\n");
const assertNothingSecret = (text) => {
  for (const secret of [ACCOUNT, AUD_DASH, AUD_QUERY, EMAIL, TEAM, TOKEN_ID, "app-", "pol-"]) {
    assert.ok(!text.includes(secret), `printed ${secret}: ${text}`);
  }
};

test("first deploy: both applications are created, with the email and the service token", async () => {
  const state = { apps: [], policies: {}, tokens: [{ id: TOKEN_ID, name: SERVICE_TOKEN }, { id: "x", name: "other" }] };
  const { api, calls } = fakeApi(state);
  const result = await setUpAccess(api, env());
  assert.deepEqual(result.vars, { ACCESS_TEAM: TEAM, DASH_AUD: AUD_DASH, QUERY_AUD: AUD_QUERY });
  assert.deepEqual(state.apps.map((app) => [app.name, app.domain, app.type]), [
    ["clauseo-counts-dash", "t.clauseo.chat/dash", "self_hosted"],
    ["clauseo-counts-query", "t.clauseo.chat/v1/query", "self_hosted"],
  ]);
  const [dash, query] = state.apps.map((app) => state.policies[app.id]);
  assert.deepEqual(dash.map((p) => [p.decision, p.include]), [["allow", [{ email: { email: EMAIL } }]]]);
  assert.deepEqual(query.map((p) => [p.decision, p.include]), [["non_identity", [{ service_token: { token_id: TOKEN_ID } }]]]);
  assert.ok(calls.includes("GET /accounts"), "the one account is found");
  assertNothingSecret(printed(result));
  assert.deepEqual(varArgs(result.vars), [
    "--var",
    `ACCESS_TEAM:${TEAM}`,
    "--var",
    `DASH_AUD:${AUD_DASH}`,
    "--var",
    `QUERY_AUD:${AUD_QUERY}`,
  ]);
});

test("a second deploy changes nothing; a new email updates the one policy", async () => {
  const state = { apps: [], policies: {}, tokens: [{ id: TOKEN_ID, name: SERVICE_TOKEN }] };
  await setUpAccess(fakeApi(state).api, env({ CLOUDFLARE_ACCOUNT_ID: ACCOUNT }));
  const again = fakeApi(state);
  const result = await setUpAccess(again.api, env({ CLOUDFLARE_ACCOUNT_ID: ACCOUNT }));
  assert.ok(again.calls.every((call) => call.startsWith("GET")), again.calls.join("\n"));
  assert.ok(!again.calls.includes("GET /accounts"), "CLOUDFLARE_ACCOUNT_ID is used when set");
  assert.equal(result.vars.DASH_AUD, AUD_DASH);
  const moved = fakeApi(state);
  await setUpAccess(moved.api, env({ CLOUDFLARE_ACCOUNT_ID: ACCOUNT, ACCESS_DASH_EMAIL: "new@example.org" }));
  assert.deepEqual(moved.calls.filter((call) => !call.startsWith("GET")), ["PUT /accounts/<account>/access/apps/<app>/policies/<policy>"]);
  const dash = state.policies[state.apps[0].id];
  assert.equal(dash.length, 1);
  assert.deepEqual(dash[0].include, [{ email: { email: "new@example.org" } }]);
});

test("without the service token, the query application has no policy and lets no one in", async () => {
  const state = { apps: [], policies: {}, tokens: [] };
  const result = await setUpAccess(fakeApi(state).api, env());
  assert.equal(result.vars.QUERY_AUD, AUD_QUERY);
  assert.deepEqual(state.policies[state.apps[1].id] ?? [], []);
  assert.match(printed(result), /clauseo-counts-query has no policy of ours yet, so it lets no one in/);
  // The same for the dashboard without an email.
  const noEmail = { apps: [], policies: {}, tokens: [] };
  await setUpAccess(fakeApi(noEmail).api, env({ ACCESS_DASH_EMAIL: "" }));
  assert.deepEqual(noEmail.policies[noEmail.apps[0].id] ?? [], []);
});

test("no ACCESS_TEAM, or a token without the scopes: skipped with one line, and no vars", async () => {
  const none = fakeApi({ apps: [], policies: {}, tokens: [] });
  const skipped = await setUpAccess(none.api, env({ ACCESS_TEAM: "" }));
  assert.equal(skipped.vars, null);
  assert.equal(skipped.lines.length, 1);
  assert.deepEqual(none.calls, [], "no API call");
  for (const status of [401, 403]) {
    const result = await setUpAccess(fakeApi({}, { deny: status }).api, env({ CLOUDFLARE_ACCOUNT_ID: ACCOUNT }));
    assert.equal(result.vars, null, String(status));
    assert.equal(result.lines.length, 1);
    assert.match(result.lines[0], new RegExp(`answered ${status}`));
    assertNothingSecret(printed(result));
    assert.deepEqual(varArgs(result.vars), []);
  }
});

test("a wrong domain, a duplicate name, another error or a bad team fails without an id", async () => {
  const moved = { apps: [{ id: "app-7", name: APPS[0].name, domain: "t.clauseo.chat/other", aud: AUD_DASH }], policies: {}, tokens: [] };
  await assert.rejects(setUpAccess(fakeApi(moved).api, env()), (error) => error instanceof Refused && /not on t.clauseo.chat\/dash/.test(error.message));
  const twice = { apps: [{ id: "app-1", name: APPS[0].name }, { id: "app-2", name: APPS[0].name }], policies: {}, tokens: [] };
  await assert.rejects(setUpAccess(fakeApi(twice).api, env()), /more than one/);
  await assert.rejects(setUpAccess(fakeApi({}, { deny: 500 }).api, env({ CLOUDFLARE_ACCOUNT_ID: ACCOUNT })), (error) => {
    assertNothingSecret(error.message);
    return error instanceof Refused && /<id>/.test(error.message);
  });
  await assert.rejects(setUpAccess(fakeApi({}).api, env({ ACCESS_TEAM: "evil.example/x" })), Refused);
});

test("the API caller sends the token as a bearer and reads the JSON", async () => {
  const seen = [];
  const fakeFetch = async (url, init) => {
    seen.push([url, init.method, init.headers.Authorization, init.body]);
    return { status: 200, json: async () => ({ success: true, result: [] }) };
  };
  const result = await cloudflareApi("tok", fakeFetch)("POST", "/x", { a: 1 });
  assert.deepEqual(result, { status: 200, json: { success: true, result: [] } });
  assert.deepEqual(seen, [["https://api.cloudflare.com/client/v4/x", "POST", "Bearer tok", '{"a":1}']]);
});

test("the redactor masks the AUD tags, the team and the email in wrangler's output", () => {
  const output = [
    "Your Worker has access to the following bindings:",
    `env.ACCESS_TEAM ("${TEAM}")`,
    `env.DASH_AUD ("${AUD_DASH}")`,
    `env.QUERY_AUD ("${AUD_QUERY}")`,
  ].join("\n");
  const shown = redact(output, [TEAM, AUD_DASH, AUD_QUERY, EMAIL]);
  assertNothingSecret(shown);
  // A 64-hex tag is masked even when not named.
  assert.ok(!redact(`x ${AUD_DASH} y`).includes(AUD_DASH));
});
