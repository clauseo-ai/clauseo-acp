// node --test scripts/check-access.test.mjs: the live Access check's offline parts. The JWT
// decode and the common_name comparison on a fixture token, the cookie and refusal readers, and
// a whole run against a stand-in fetch and Keychain, which prints only yes/no lines and never the
// client id, the secret, the JWT or where Access redirects.

import { test } from "node:test";
import assert from "node:assert/strict";
import { checkAccess, commonNameMatches, cookie, jwtClaims, ORIGIN, QUERY, refusedByAccess } from "./check-access.mjs";

const CLIENT_ID = "0123456789abcdef0123456789abcdef.access";
const SECRET = "s".repeat(64);
const b64 = (value) => Buffer.from(JSON.stringify(value)).toString("base64url");
/** A fixture of the JWT Access sets for a service-token request (signature not checked here). */
const fixture = (claims) => `${b64({ alg: "RS256", kid: "k1" })}.${b64(claims)}.c2lnbmF0dXJl`;
const SERVICE_CLAIMS = { aud: ["aud"], iss: "https://team.cloudflareaccess.com", type: "app", sub: "", common_name: CLIENT_ID };

test("the fixture JWT decodes, and common_name is compared exactly", () => {
  const claims = jwtClaims(fixture(SERVICE_CLAIMS));
  assert.equal(claims.common_name, CLIENT_ID);
  assert.equal(commonNameMatches(claims, CLIENT_ID), true);
  assert.equal(commonNameMatches(claims, CLIENT_ID.replace(".access", "")), false, "no suffix");
  assert.equal(commonNameMatches({ common_name: CLIENT_ID.replace(".access", "") }, CLIENT_ID), false);
  assert.equal(commonNameMatches({ common_name: CLIENT_ID.toUpperCase() }, CLIENT_ID), false);
  assert.equal(commonNameMatches({ email: "x@example.org" }, CLIENT_ID), false);
  assert.equal(commonNameMatches(null, CLIENT_ID), false);
  for (const bad of [undefined, "", "a.b", "a.b.c.d", "x.!!.y", `x.${Buffer.from("[1]").toString("base64url")}.y`]) {
    assert.equal(jwtClaims(bad), null, String(bad));
  }
});

test("the cookie and refusal readers", () => {
  const headers = ["other=1; Path=/", "CF_Authorization=abc.def.ghi; Path=/; Secure; HttpOnly"];
  assert.equal(cookie(headers, "CF_Authorization"), "abc.def.ghi");
  assert.equal(cookie(headers, "missing"), undefined);
  assert.equal(cookie(undefined, "CF_Authorization"), undefined);
  assert.equal(refusedByAccess(302, "https://team.cloudflareaccess.com/cdn-cgi/access/login/x", ""), true);
  assert.equal(refusedByAccess(302, "https://evil.example/cloudflareaccess.com", ""), false);
  assert.equal(refusedByAccess(302, undefined, ""), false);
  assert.equal(refusedByAccess(403, null, "<html>Forbidden</html>"), true);
  assert.equal(refusedByAccess(403, null, ""), false, "the Worker's own 403 is empty");
  assert.equal(refusedByAccess(404, null, ""), false, "the Worker answered");
  assert.equal(refusedByAccess(200, null, "[]"), false);
});

/** A stand-in for the edge: Access in front, and the Worker behind it for the real token. */
function edge({ cookieClaims = SERVICE_CLAIMS, open = false } = {}) {
  const seen = [];
  const fetchImpl = async (url, init) => {
    seen.push({ url, init });
    const id = init.headers?.["CF-Access-Client-Id"];
    const secret = init.headers?.["CF-Access-Client-Secret"];
    if (open || (id === CLIENT_ID && secret === SECRET)) {
      const headers = new Headers({ "content-type": "application/json" });
      headers.append("set-cookie", `CF_Authorization=${fixture(cookieClaims)}; Path=/; Secure`);
      return new Response("[]", { status: 200, headers });
    }
    if (id !== undefined) return new Response("<html>Forbidden</html>", { status: 403 });
    return new Response(null, { status: 302, headers: { location: "https://team.cloudflareaccess.com/login?id=abc" } });
  };
  return { fetchImpl, seen };
}

const runWith = async (options) => {
  const printed = [];
  const ok = await checkAccess({ ...options, print: (line) => printed.push(line) });
  return { ok, printed, text: printed.join("\n") };
};

const assertNothingSecret = (text) => {
  for (const secret of [CLIENT_ID, SECRET, "cloudflareaccess", "id=abc", "eyJ", "CF_Authorization="]) {
    assert.ok(!text.includes(secret), `printed ${secret}`);
  }
};

test("a whole run against a set-up edge says yes to every line", async () => {
  const { fetchImpl, seen } = edge();
  const { ok, printed, text } = await runWith({ fetchImpl, readKeychain: () => ({ clientId: CLIENT_ID, clientSecret: SECRET }) });
  assert.equal(ok, true, text);
  assert.equal(printed.filter((line) => line.endsWith(": yes")).length, 5, text);
  assert.match(text, /by hand: open https:\/\/t\.clauseo\.chat\/dash and sign in/);
  assertNothingSecret(text);
  assert.ok(seen.every(({ url, init }) => url.startsWith(ORIGIN) && init.redirect === "manual"));
  const real = seen.find(({ init }) => init.headers?.["CF-Access-Client-Id"] === CLIENT_ID);
  assert.deepEqual(JSON.parse(real.init.body), { sql: QUERY });
});

test("an open route, a wrong common_name or no token in the Keychain says so", async () => {
  const open = await runWith({ fetchImpl: edge({ open: true }).fetchImpl, readKeychain: () => null });
  assert.equal(open.ok, false);
  assert.equal(open.printed.filter((line) => line.endsWith(": no")).length, 3, open.text);
  assert.match(open.text, /not in the Keychain/);
  assertNothingSecret(open.text);

  const wrong = await runWith({
    fetchImpl: edge({ cookieClaims: { ...SERVICE_CLAIMS, common_name: "other.access" } }).fetchImpl,
    readKeychain: () => ({ clientId: CLIENT_ID, clientSecret: SECRET }),
  });
  assert.equal(wrong.ok, false);
  assert.match(wrong.text, /common_name equals the token's client id \(with its \.access suffix\): no/);
  assertNothingSecret(wrong.text);

  const down = await runWith({
    fetchImpl: async () => {
      throw new Error(`network ${CLIENT_ID}`);
    },
    readKeychain: () => ({ clientId: CLIENT_ID, clientSecret: SECRET }),
  });
  assert.equal(down.ok, false);
  assertNothingSecret(down.text);
});
