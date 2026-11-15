// The Access step of the counts deploy (docs/counts-worker.md, "Access"). /dash and /v1/query
// answer 404 unless the Worker knows the Access team and each route's application AUD, and those
// reach it at deploy as `wrangler deploy --var`, never through wrangler.toml. This module finds or
// creates the two self-hosted Access applications by name, keeps exactly one policy on each, and
// returns the values to pass:
//
// - clauseo-counts-dash on t.clauseo.chat/dash: Allow, for the email in ACCESS_DASH_EMAIL, which
//   also reaches the Worker as DASH_EMAIL: /dash takes only a token with that email;
// - clauseo-counts-query on t.clauseo.chat/v1/query: Service Auth, for the service token named
//   clauseo-counts-agent, whose client id (not its secret) reaches the Worker as QUERY_CLIENT_ID:
//   /v1/query takes only a token with that common_name. While that token does not exist the
//   application has no policy of ours, so it lets no one in, and the Worker answers 404.
//
// Any other policy on the two applications, one added by hand or a second of our name included,
// is deleted, so each application ends every deploy with exactly one policy, ours (or none).
// Every read comes first, and its shape is checked before anything changes: a list where one is
// read, policies with string `id`, `name` and `decision` and a boolean `reusable`, and
// `app.policies` a list of the same. Any other shape skips Access like a 401 or 403. A reusable
// policy (shared between applications) is not deleted from here: the step fails and names the
// application, to be fixed by hand. The Worker's own check of the caller holds either way.
//
// Run logs on this public repo are public: nothing here prints an id, an AUD, the team or an
// email. Errors name the API path's shape, not its ids. Without ACCESS_TEAM, or when the API
// answers 401 or 403 (the token lacks the Access scopes), the step is skipped with one line and
// the deploy goes on: both routes then stay 404.

import { Refused } from "./errors.mjs";

export const API = "https://api.cloudflare.com/client/v4";
export const SERVICE_TOKEN = "clauseo-counts-agent";

/** The two applications, the variable each one's AUD reaches the Worker as, and its caller's. */
export const APPS = [
  { name: "clauseo-counts-dash", domain: "t.clauseo.chat/dash", variable: "DASH_AUD", callerVariable: "DASH_EMAIL" },
  {
    name: "clauseo-counts-query",
    domain: "t.clauseo.chat/v1/query",
    variable: "QUERY_AUD",
    callerVariable: "QUERY_CLIENT_ID",
  },
];

/** The API answered 401 or 403: the token has no Access scopes yet. */
export class AccessUnavailable extends Error {}

/** The API answered in a shape this step does not read; the message names what, never an id. */
export class AccessShape extends Error {}

/** `value` if it is a list, else AccessShape naming `what`. */
function expectList(value, what) {
  if (!Array.isArray(value)) throw new AccessShape(what);
  return value;
}

/**
 * `value` if it is a list of policies with the fields this step reads (`id` and `name` strings,
 * `reusable` a boolean, `decision` a string), else AccessShape naming `what`.
 */
export function expectPolicies(value, what) {
  const ok = (p) =>
    p !== null &&
    typeof p === "object" &&
    typeof p.id === "string" &&
    typeof p.name === "string" &&
    typeof p.reusable === "boolean" &&
    typeof p.decision === "string";
  if (!Array.isArray(value) || !value.every(ok)) throw new AccessShape(what);
  return value;
}

/**
 * A caller of Cloudflare's API with `token`: `(method, path, body?) => { status, json }`.
 * @param {string} token
 * @param {typeof fetch} fetchImpl
 */
export function cloudflareApi(token, fetchImpl = fetch) {
  return async (method, path, body) => {
    const response = await fetchImpl(API + path, {
      method,
      headers: { Authorization: `Bearer ${token}`, "Content-Type": "application/json" },
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

/** `path` with every id-like segment replaced, safe to print. */
function shape(path) {
  return path.replace(/\/[0-9a-f-]{16,}(?=\/|$|\?)/gi, "/<id>").replace(/\?.*$/, "");
}

async function call(api, method, path, body) {
  const { status, json } = await api(method, path, body);
  if (status === 401 || status === 403) throw new AccessUnavailable(String(status));
  if (status < 200 || status >= 300 || json?.success !== true) {
    throw new Refused(`the Cloudflare API refused ${method} ${shape(path)} (${status})`);
  }
  return json.result;
}

/** The account: CLOUDFLARE_ACCOUNT_ID, or the one account the token sees. */
async function accountId(api, env) {
  if (env.CLOUDFLARE_ACCOUNT_ID) return env.CLOUDFLARE_ACCOUNT_ID;
  const accounts = await call(api, "GET", "/accounts");
  if (!Array.isArray(accounts) || accounts.length !== 1) {
    throw new Refused("the token sees more or fewer than one account; set CLOUDFLARE_ACCOUNT_ID");
  }
  return accounts[0].id;
}

/** The caller each application's Worker route takes, or undefined when there is none yet. */
function caller(app, env, token) {
  if (app.variable === "DASH_AUD") return env.ACCESS_DASH_EMAIL || undefined;
  return token?.client_id || undefined;
}

/** The policy each application must have, or undefined when it should have none of ours. */
function wantedPolicy(app, env, token) {
  if (app.variable === "DASH_AUD") {
    if (!env.ACCESS_DASH_EMAIL) return undefined;
    return { name: `${app.name} owner`, decision: "allow", include: [{ email: { email: env.ACCESS_DASH_EMAIL } }] };
  }
  if (token === undefined || !token.client_id) return undefined;
  return { name: `${app.name} agent`, decision: "non_identity", include: [{ service_token: { token_id: token.id } }] };
}

const sameRules = (a, b) => JSON.stringify(a ?? []) === JSON.stringify(b ?? []);

/**
 * Finds or creates the applications and their policies.
 * @returns {Promise<{ vars: Record<string, string> | null, lines: string[] }>} the --var values
 *   (null when skipped) and the lines to print, which hold no id.
 */
export async function setUpAccess(api, env) {
  const team = env.ACCESS_TEAM ?? "";
  if (team === "") return { vars: null, lines: ["Access: skipped, ACCESS_TEAM is not set; /dash and /v1/query stay 404"] };
  if (!/^[a-z0-9-]{1,63}$/.test(team)) throw new Refused("ACCESS_TEAM is not a plain team name");
  const lines = [];
  const vars = { ACCESS_TEAM: team };
  try {
    const base = `/accounts/${await accountId(api, env)}/access`;
    // First every read and its shape check, so nothing changes unless all of it is as expected.
    const apps = expectList(await call(api, "GET", `${base}/apps?per_page=1000`), "application list");
    const tokens = expectList(await call(api, "GET", `${base}/service_tokens?per_page=1000`), "service token list");
    const token = tokens.find((t) => t?.name === SERVICE_TOKEN);
    if (token !== undefined && (typeof token.id !== "string" || typeof token.client_id !== "string")) {
      throw new AccessShape("service token");
    }
    const plans = [];
    for (const wanted of APPS) {
      const found = apps.filter((app) => app?.name === wanted.name);
      if (found.length > 1) throw new Refused(`more than one Access application is named ${wanted.name}`);
      const app = found[0];
      let policies = [];
      if (app !== undefined) {
        if (typeof app.id !== "string") throw new AccessShape("application");
        if (app.domain !== wanted.domain) {
          throw new Refused(`the Access application ${wanted.name} is not on ${wanted.domain}; fix it by hand`);
        }
        if (typeof app.aud !== "string" || app.aud === "") throw new Refused(`${wanted.name} has no AUD tag`);
        // The application's own list, and the ones the application names (reusable ones too).
        const attached = expectPolicies(app.policies, "application's policy list");
        policies = expectPolicies(await call(api, "GET", `${base}/apps/${app.id}/policies`), "policy list");
        const shared = [...policies, ...attached].some(
          (p) => p.reusable || !policies.some((own) => own.id === p.id),
        );
        if (shared) throw new Refused(`${wanted.name} has a reusable Access policy attached; remove it by hand`);
      }
      plans.push({ wanted, app, policies });
    }
    // Then the changes: each application ends with exactly one policy, ours, or none.
    for (const { wanted, app: found, policies } of plans) {
      let app = found;
      if (app === undefined) {
        app = await call(api, "POST", `${base}/apps`, {
          name: wanted.name,
          domain: wanted.domain,
          type: "self_hosted",
          session_duration: "24h",
          app_launcher_visible: false,
        });
        if (typeof app?.id !== "string" || typeof app.aud !== "string" || app.aud === "") {
          throw new AccessShape("created application");
        }
        lines.push(`Access: created ${wanted.name}`);
      }
      const policy = wantedPolicy(wanted, env, token);
      // The first policy of our name is kept; any other, a second of the same name included, goes.
      const kept = policy === undefined ? undefined : policies.find((p) => p.name === policy.name);
      if (policy === undefined) {
        lines.push(`Access: ${wanted.name} has no policy of ours yet, so it lets no one in`);
      } else if (kept === undefined) {
        await call(api, "POST", `${base}/apps/${app.id}/policies`, { ...policy, precedence: 1 });
        lines.push(`Access: ${wanted.name}'s policy created`);
      } else if (kept.decision !== policy.decision || !sameRules(kept.include, policy.include)) {
        await call(api, "PUT", `${base}/apps/${app.id}/policies/${kept.id}`, { ...policy, precedence: kept.precedence ?? 1 });
        lines.push(`Access: ${wanted.name}'s policy updated`);
      }
      const others = policies.filter((p) => p.id !== kept?.id);
      for (const other of others) await call(api, "DELETE", `${base}/apps/${app.id}/policies/${other.id}`);
      if (others.length > 0) lines.push(`Access: ${wanted.name}: ${others.length} other policy(ies) deleted`);
      vars[wanted.variable] = app.aud;
      const who = caller(wanted, env, token);
      if (who !== undefined) vars[wanted.callerVariable] = who;
    }
  } catch (error) {
    if (error instanceof AccessShape) {
      return {
        vars: null,
        lines: [`Access: skipped, the API's ${error.message} is not in the shape this step reads; /dash and /v1/query stay 404`],
      };
    }
    if (!(error instanceof AccessUnavailable)) throw error;
    return {
      vars: null,
      lines: [`Access: skipped, the API answered ${error.message} (the token lacks the Access scopes); /dash and /v1/query stay 404`],
    };
  }
  const callers = APPS.filter((app) => vars[app.callerVariable] !== undefined).map((app) => app.callerVariable);
  lines.push(
    `Access: the team, both AUD tags and ${callers.length === 0 ? "no caller" : callers.join(" and ")} go to the Worker as deploy variables`,
  );
  return { vars, lines };
}

/** `wrangler deploy`'s `--var` arguments for `vars`. */
export function varArgs(vars) {
  return Object.entries(vars ?? {}).flatMap(([name, value]) => ["--var", `${name}:${value}`]);
}
