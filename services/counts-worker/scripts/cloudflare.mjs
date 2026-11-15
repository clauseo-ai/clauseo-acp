// A small caller of Cloudflare's API for the deploy job's scripts, with the token from the
// environment. Run logs on this public repo are public, so nothing here prints: errors name the
// path's shape, with every id-like segment replaced, never a response body.

import { Refused } from "./errors.mjs";

export const API = "https://api.cloudflare.com/client/v4";

/** The API answered 401 or 403: the token lacks a permission, which a step may skip on. */
export class NotPermitted extends Error {}

/**
 * A caller of Cloudflare's API with `token`: `(method, path, body?, headers?) => { status, json }`.
 * @param {string} token
 * @param {typeof fetch} fetchImpl
 */
export function cloudflareApi(token, fetchImpl = fetch) {
  return async (method, path, body, headers = {}) => {
    const response = await fetchImpl(API + path, {
      method,
      headers: { Authorization: `Bearer ${token}`, "Content-Type": "application/json", ...headers },
      body: body === undefined ? undefined : JSON.stringify(body),
    });
    let json = null;
    try {
      json = await response.json();
    } catch {
      json = null;
    }
    return { status: response.status, json };
  };
}

/** `path` with every id-like segment and the query replaced, safe to print. */
export function shape(path) {
  return path.replace(/\/[0-9a-f-]{16,}(?=\/|$|\?)/gi, "/<id>").replace(/\?.*$/, "");
}

/**
 * One call: its `result` on success; `NotPermitted` on 401 or 403; `Refused` otherwise, unless
 * `status` is in `allow`, which returns `{ status }` instead.
 */
export async function call(api, method, path, { body, headers, allow = [] } = {}) {
  const { status, json } = await api(method, path, body, headers);
  if (status === 401 || status === 403) throw new NotPermitted(String(status));
  if (allow.includes(status)) return { status };
  if (status < 200 || status >= 300 || json?.success !== true) {
    throw new Refused(`the Cloudflare API refused ${method} ${shape(path)} (${status})`);
  }
  return json.result;
}

/** The account: CLOUDFLARE_ACCOUNT_ID, or the one account the token sees. */
export async function accountId(api, env) {
  if (env.CLOUDFLARE_ACCOUNT_ID) return env.CLOUDFLARE_ACCOUNT_ID;
  const accounts = await call(api, "GET", "/accounts");
  if (!Array.isArray(accounts) || accounts.length !== 1) {
    throw new Refused("the token sees more or fewer than one account; set CLOUDFLARE_ACCOUNT_ID");
  }
  return accounts[0].id;
}
