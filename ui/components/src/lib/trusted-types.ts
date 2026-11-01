// The one home of the Trusted Types policy. The CSP (crates/desktop/tauri.conf.json) enforces Trusted Types
// (`require-trusted-types-for 'script'`) and allows exactly one policy name, `trusted-types clauseo-sanitize`, without
// 'allow-duplicates', so the policy is created once, here, and a second createPolicy would throw. The policy object
// never leaves this module.
//
// It has one rule, createScriptURL, for starting a Worker: a script URL is accepted only from the page's own scheme
// and host, and everything else is refused (blob:, data:, javascript:, any other origin). There is no path rule: the
// page's origin serves only our own build (Tauri's protocol serves the bundled dist; in development Vite serves our
// sources from /src/, /@fs/ and /node_modules/.vite/, which a path rule would have to list), so a path would add a
// second list to keep without narrowing who can supply the script. `worker-src 'self'` says the same; the policy is
// what stops a string an attacker built from reaching the Worker constructor at all.
//
// No createHTML and no createScript: nothing in the app turns a string into HTML or code (ui/app/src/csp.test.ts).

/** What a Worker URL becomes: a TrustedScriptURL where Trusted Types exists, else the checked URL itself. Typed as a
 * string because that is what `new Worker()` declares; the engine reads the trusted value. */
export type ScriptUrl = string & { readonly __scriptUrl: unique symbol };

interface Policy {
  createScriptURL(url: string): unknown;
}
interface TrustedTypesLike {
  createPolicy(name: string, rules: { createScriptURL: (url: string) => string }): Policy;
}

export const POLICY_NAME = "clauseo-sanitize";

/** The URL, made absolute against the page, when it may start a script; throws a TypeError otherwise. */
export function checkScriptUrl(url: string): string {
  const page = globalThis.location;
  if (!page) throw new TypeError("no page to compare a script URL with");
  let parsed: URL;
  try {
    parsed = new URL(url, page.href);
  } catch {
    throw new TypeError("refused script URL: not a URL");
  }
  // Scheme and host, not URL.origin: the page's tauri://localhost has an opaque origin ("null") by the URL standard,
  // and two opaque origins never compare equal. A blob: or data: URL never has the page's scheme.
  const same = parsed.protocol === page.protocol && parsed.host === page.host && parsed.host !== "";
  if (!same || parsed.username !== "" || parsed.password !== "") {
    throw new TypeError(`refused script URL from ${parsed.protocol}`);
  }
  return parsed.href;
}

/** undefined: not asked yet; null: the engine has no Trusted Types. */
let policy: Policy | null | undefined;

/** The URL a Worker may start from, through the `clauseo-sanitize` policy. Throws for a URL the rule refuses, and
 * when the policy cannot be created (a second copy of this module, as a hot reload in development makes). */
export function scriptUrl(url: string): ScriptUrl {
  if (policy === undefined) {
    const types = (globalThis as { trustedTypes?: TrustedTypesLike }).trustedTypes;
    policy = types ? types.createPolicy(POLICY_NAME, { createScriptURL: checkScriptUrl }) : null;
  }
  // Checked here as well, so an engine without Trusted Types refuses the same URLs.
  const checked = checkScriptUrl(url);
  return (policy ? policy.createScriptURL(checked) : checked) as ScriptUrl;
}
