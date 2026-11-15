// The Access step of the counts deploy (docs/counts-worker.md, "Access"). /dash and /v1/query
// answer 404 unless the Worker knows the Access team and each route's application AUD, and those
// reach it at deploy as `wrangler deploy --var`, never through wrangler.toml. This module finds or
// creates the two self-hosted Access applications by name, keeps their one policy each, and
// returns the values to pass:
//
// - clauseo-counts-dash on t.clauseo.chat/dash: Allow, for the email in ACCESS_DASH_EMAIL;
// - clauseo-counts-query on t.clauseo.chat/v1/query: Service Auth, for the service token named
//   clauseo-counts-agent. While that token does not exist the application has no policy of ours,
//   so it lets no one in.
//
// Run logs on this public repo are public: nothing here prints an id, an AUD, the team or an
// email. Errors name the API path's shape, not its ids. Without ACCESS_TEAM, or when the API
// answers 401 or 403 (the token lacks the Access scopes), the step is skipped with one line and
// the deploy goes on: both routes then stay 404.

import { Refused } from "./errors.mjs";

export const API = "https://api.cloudflare.com/client/v4";
export const SERVICE_TOKEN = "clauseo-counts-agent";

/** The two applications, and the variable each one's AUD reaches the Worker as. */
export const APPS = [
  { name: "clauseo-counts-dash", domain: "t.clauseo.chat/dash", variable: "DASH_AUD" },
  { name: "clauseo-counts-query", domain: "t.clauseo.chat/v1/query", variable: "QUERY_AUD" },
];

/** The API answered 401 or 403: the token has no Access scopes yet. */
export class AccessUnavailable extends Error {}

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

/** The policy each application must have, or undefined when it should have none of ours. */
function wantedPolicy(app, env, token) {
  if (app.variable === "DASH_AUD") {
    if (!env.ACCESS_DASH_EMAIL) return undefined;
    return { name: `${app.name} owner`, decision: "allow", include: [{ email: { email: env.ACCESS_DASH_EMAIL } }] };
  }
  if (token === undefined) return undefined;
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
    const apps = (await call(api, "GET", `${base}/apps?per_page=1000`)) ?? [];
    const tokens = (await call(api, "GET", `${base}/service_tokens?per_page=1000`)) ?? [];
    const token = tokens.find((t) => t.name === SERVICE_TOKEN);
    for (const wanted of APPS) {
      const found = apps.filter((app) => app.name === wanted.name);
      if (found.length > 1) throw new Refused(`more than one Access application is named ${wanted.name}`);
      let app = found[0];
      if (app === undefined) {
        app = await call(api, "POST", `${base}/apps`, {
          name: wanted.name,
          domain: wanted.domain,
          type: "self_hosted",
          session_duration: "24h",
          app_launcher_visible: false,
        });
        lines.push(`Access: created ${wanted.name}`);
      }
      if (app.domain !== wanted.domain) {
        throw new Refused(`the Access application ${wanted.name} is not on ${wanted.domain}; fix it by hand`);
      }
      if (typeof app.aud !== "string" || app.aud === "") throw new Refused(`${wanted.name} has no AUD tag`);
      const policies = (await call(api, "GET", `${base}/apps/${app.id}/policies`)) ?? [];
      const policy = wantedPolicy(wanted, env, token);
      if (policy === undefined) {
        lines.push(`Access: ${wanted.name} has no policy of ours yet, so it lets no one in`);
      } else {
        const existing = policies.find((p) => p.name === policy.name);
        if (existing === undefined) {
          await call(api, "POST", `${base}/apps/${app.id}/policies`, { ...policy, precedence: 1 });
          lines.push(`Access: ${wanted.name}'s policy created`);
        } else if (existing.decision !== policy.decision || !sameRules(existing.include, policy.include)) {
          await call(api, "PUT", `${base}/apps/${app.id}/policies/${existing.id}`, { ...policy, precedence: existing.precedence ?? 1 });
          lines.push(`Access: ${wanted.name}'s policy updated`);
        }
      }
      const others = policies.filter((p) => p.name !== policy?.name).length;
      if (others > 0) lines.push(`Access: ${wanted.name} has ${others} other policy(ies), left as they are`);
      vars[wanted.variable] = app.aud;
    }
  } catch (error) {
    if (!(error instanceof AccessUnavailable)) throw error;
    return {
      vars: null,
      lines: [`Access: skipped, the API answered ${error.message} (the token lacks the Access scopes); /dash and /v1/query stay 404`],
    };
  }
  lines.push("Access: the team and both AUD tags go to the Worker as deploy variables");
  return { vars, lines };
}

/** `wrangler deploy`'s `--var` arguments for `vars`. */
export function varArgs(vars) {
  return Object.entries(vars ?? {}).flatMap(([name, value]) => ["--var", `${name}:${value}`]);
}
