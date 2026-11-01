// The one home of the Trusted Types policy. The CSP (crates/desktop/tauri.conf.json) enforces Trusted Types
// (`require-trusted-types-for 'script'`) and allows exactly one policy name, `trusted-types clauseo-sanitize`, without
// 'allow-duplicates', so the policy is created once, here, and a second createPolicy would throw. The policy object
// never leaves this module.
//
// It has one rule, createScriptURL, for starting a Worker: it accepts exactly the code-colouring worker's own URL
// (Vite's `?worker&url`: the built /assets/worker-*.js in production, the served source in development, so one rule
// holds in both), resolved against the page and on the page's own scheme and host. Everything else is refused: any
// other path on our origin, blob:, data:, javascript:, any other origin. A new worker adds its URL to SCRIPTS.
//
// No createHTML and no createScript: nothing in the app turns a string into HTML or code (ui/app/src/csp.test.ts).

// Vite's `?worker&url` module has one default export, the URL (vite/client declares it); the import plugin resolves
// the path to worker.ts itself, which has none.
// oxlint-disable-next-line import/default
import highlightWorker from "../markdown/highlight/worker.ts?worker&url";

/** The only scripts the policy lets start: the URLs Vite gives our workers. */
const SCRIPTS: readonly string[] = [highlightWorker];

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

/** The URL, made absolute against the page, when it is one of SCRIPTS; throws a TypeError otherwise. */
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
  if (!SCRIPTS.some((script) => new URL(script, page.href).href === parsed.href)) {
    throw new TypeError("refused script URL: not one of the app's workers");
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
