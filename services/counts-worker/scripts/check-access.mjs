#!/usr/bin/env node
// The live Access check (docs/counts-worker.md, "Checking Access live"), run on Rohan's Mac after
// the one-time set-up and before the deploy that turns Access on is approved:
//
//   node services/counts-worker/scripts/check-access.mjs
//
// It prints only yes/no lines and the one step to do by hand: never a token, an email, an id, a
// URL Access redirects to, or a JWT. The service token's two halves are read from the Keychain
// (service clauseo-counts-query, accounts client-id and client-secret) at call time and only
// sent as Access's headers. It exits 1 when any line says no.

import { execFileSync } from "node:child_process";
import { randomBytes } from "node:crypto";
import { fileURLToPath } from "node:url";

export const ORIGIN = "https://t.clauseo.chat";
export const KEYCHAIN_SERVICE = "clauseo-counts-query";
export const QUERY = "SELECT 1 AS one FROM v_active LIMIT 1";

/** A JWT's claims, or null when it is not three base64url parts with a JSON object between. */
export function jwtClaims(jwt) {
  if (typeof jwt !== "string") return null;
  const parts = jwt.split(".");
  if (parts.length !== 3 || !/^[A-Za-z0-9_-]+$/.test(parts[1])) return null;
  try {
    const claims = JSON.parse(Buffer.from(parts[1], "base64url").toString("utf8"));
    return claims !== null && typeof claims === "object" && !Array.isArray(claims) ? claims : null;
  } catch {
    return null;
  }
}

/**
 * Whether the claims' common_name is exactly the service token's client id, which carries its
 * `.access` suffix as Cloudflare issues it.
 */
export function commonNameMatches(claims, clientId) {
  return (
    typeof clientId === "string" &&
    clientId.endsWith(".access") &&
    typeof claims?.common_name === "string" &&
    claims.common_name === clientId
  );
}

/** The value of cookie `name` among `Set-Cookie` header values, or undefined. */
export function cookie(setCookies, name) {
  for (const header of setCookies ?? []) {
    const [pair] = header.split(";");
    const at = pair.indexOf("=");
    if (at > 0 && pair.slice(0, at).trim() === name) return pair.slice(at + 1).trim();
  }
  return undefined;
}

/**
 * Whether Access answered instead of the Worker: a 302 to a `*.cloudflareaccess.com` login, or a
 * 403 with a body (Access's page; the Worker's 403 and 404 have empty bodies).
 */
export function refusedByAccess(status, location, body) {
  if (status === 302 || status === 303) {
    try {
      return new URL(location ?? "").hostname.endsWith(".cloudflareaccess.com");
    } catch {
      return false;
    }
  }
  return status === 403 && typeof body === "string" && body.trim() !== "";
}

/** The service token's halves from the Keychain, or null when they are not there. */
function keychain() {
  const read = (account) =>
    execFileSync("security", ["find-generic-password", "-s", KEYCHAIN_SERVICE, "-a", account, "-w"], {
      encoding: "utf8",
      stdio: ["ignore", "pipe", "ignore"],
    }).trim();
  try {
    return { clientId: read("client-id"), clientSecret: read("client-secret") };
  } catch {
    return null;
  }
}

const yesNo = (ok) => (ok ? "yes" : "no");

/**
 * Runs every check with `fetchImpl` and `readKeychain`, and prints each line with `print`.
 * @returns {Promise<boolean>} whether every line said yes
 */
export async function checkAccess({ fetchImpl = fetch, readKeychain = keychain, print = console.log } = {}) {
  let ok = true;
  const line = (text, passed) => {
    ok &&= passed;
    print(`${text}: ${yesNo(passed)}`);
  };
  const send = async (path, init = {}) => {
    try {
      const response = await fetchImpl(ORIGIN + path, { redirect: "manual", ...init });
      return { status: response.status, headers: response.headers, body: await response.text() };
    } catch {
      return { status: 0, headers: new Headers(), body: "" };
    }
  };
  const queryInit = (headers) => ({
    method: "POST",
    headers: { "Content-Type": "application/json", ...headers },
    body: JSON.stringify({ sql: QUERY }),
  });
  const refused = (r) => refusedByAccess(r.status, r.headers.get("location"), r.body);

  line("/dash without credentials is stopped by Access (login redirect or 403)", refused(await send("/dash")));
  line("/v1/query without credentials is stopped by Access", refused(await send("/v1/query", queryInit({}))));
  const madeUp = {
    "CF-Access-Client-Id": `${randomBytes(16).toString("hex")}.access`,
    "CF-Access-Client-Secret": randomBytes(32).toString("hex"),
  };
  line("/v1/query with a made-up service token is stopped by Access", refused(await send("/v1/query", queryInit(madeUp))));

  const token = readKeychain();
  if (token === null) {
    print(`the service token is not in the Keychain (${KEYCHAIN_SERVICE}); its two checks are skipped`);
  } else {
    const answer = await send(
      "/v1/query",
      queryInit({ "CF-Access-Client-Id": token.clientId, "CF-Access-Client-Secret": token.clientSecret }),
    );
    line("/v1/query with the service token answers 200", answer.status === 200);
    const setCookies = typeof answer.headers.getSetCookie === "function" ? answer.headers.getSetCookie() : [];
    const claims = jwtClaims(cookie(setCookies, "CF_Authorization"));
    line(
      "common_name equals the token's client id (with its .access suffix)",
      commonNameMatches(claims, token.clientId),
    );
  }
  print(
    "by hand: open https://t.clauseo.chat/dash and sign in; a page means the email matches, a 403 means it does not",
  );
  return ok;
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  checkAccess().then(
    (ok) => process.exit(ok ? 0 : 1),
    (error) => {
      console.error(`check-access failed (${error?.name ?? "error"})`);
      process.exit(1);
    },
  );
}
