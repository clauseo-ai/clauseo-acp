// The R2 step of the counts deploy (docs/counts-worker.md, "Crash reports"). /v1/crash keeps
// each report in the bucket clauseo-crash, in the EU jurisdiction. This step finds it, or
// creates it in the EU, and makes sure its lifecycle has the rule that deletes objects after 90
// days, keeping any other rule as it is. It answers whether the deploy copy may bind the bucket.
//
// Without the token's Workers R2 Storage Edit permission (the API answers 401 or 403) it skips
// with one line: the deploy copy then has no R2 binding, the deploy goes on, and /v1/crash
// answers 503. Nothing here prints an id.

import { accountId, call, NotPermitted } from "./cloudflare.mjs";

export const BUCKET = "clauseo-crash";
export const RULE_ID = "clauseo-crash-90-days";
export const MAX_AGE_SECS = 90 * 86_400;
/** Every call names the EU jurisdiction; a bucket of that name elsewhere is another bucket. */
const EU = { "cf-r2-jurisdiction": "eu" };

/** The rule this step keeps on the bucket. */
export const RULE = {
  id: RULE_ID,
  enabled: true,
  conditions: { prefix: "" },
  deleteObjectsTransition: { condition: { type: "Age", maxAge: MAX_AGE_SECS } },
};

const sameRule = (rule) => JSON.stringify(rule) === JSON.stringify(RULE);

/**
 * Finds or creates the bucket and its rule.
 * @returns {Promise<{ bound: boolean, lines: string[] }>} whether the deploy copy binds the
 *   bucket, and the lines to print, which hold no id.
 */
export async function setUpCrashBucket(api, env) {
  const lines = [];
  try {
    const base = `/accounts/${await accountId(api, env)}/r2/buckets`;
    const found = await call(api, "GET", `${base}/${BUCKET}`, { headers: EU, allow: [404] });
    if (found?.status === 404) {
      await call(api, "POST", base, { body: { name: BUCKET }, headers: EU });
      lines.push(`R2: created ${BUCKET} in the EU jurisdiction`);
    } else if (found?.jurisdiction !== undefined && found.jurisdiction !== "eu") {
      return { bound: false, lines: [`R2: ${BUCKET} is not in the EU; /v1/crash stays 503 until it is fixed by hand`] };
    }
    const lifecycle = await call(api, "GET", `${base}/${BUCKET}/lifecycle`, { headers: EU });
    const rules = Array.isArray(lifecycle?.rules) ? lifecycle.rules : [];
    if (!rules.some(sameRule)) {
      const others = rules.filter((rule) => rule?.id !== RULE_ID);
      await call(api, "PUT", `${base}/${BUCKET}/lifecycle`, { body: { rules: [...others, RULE] }, headers: EU });
      lines.push(`R2: ${BUCKET}'s 90-day lifecycle rule set`);
    }
  } catch (error) {
    if (!(error instanceof NotPermitted)) throw error;
    return {
      bound: false,
      lines: [`R2: skipped, the API answered ${error.message} (the token lacks Workers R2 Storage Edit); /v1/crash answers 503`],
    };
  }
  lines.push(`R2: the deploy binds ${BUCKET}`);
  return { bound: true, lines };
}
