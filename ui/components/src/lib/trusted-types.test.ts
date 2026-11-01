import { afterEach, beforeEach, expect, test, vi } from "vitest";
// oxlint-disable-next-line import/default
import WORKER from "../markdown/highlight/worker.ts?worker&url";

const PAGES = [
  "https://tauri.localhost/",
  "http://tauri.localhost/",
  "tauri://localhost/",
  "http://localhost:1420/",
];

/** A fresh copy of the module (its policy is created once per copy) under a page at `href`, with or without a fake
 * trustedTypes that records the policies created and wraps what the rule returns. */
async function load(href: string, withTypes: boolean) {
  const url = new URL(href);
  vi.stubGlobal("location", {
    href: url.href,
    protocol: url.protocol,
    host: url.host,
    origin: url.origin,
  });
  const created: string[] = [];
  const rules: { createScriptURL?: (url: string) => string } = {};
  if (withTypes) {
    vi.stubGlobal("trustedTypes", {
      createPolicy(name: string, given: { createScriptURL: (url: string) => string }) {
        if (created.includes(name)) throw new TypeError("duplicate policy");
        created.push(name);
        rules.createScriptURL = given.createScriptURL;
        return { createScriptURL: (input: string) => ({ trusted: given.createScriptURL(input) }) };
      },
    });
  }
  vi.resetModules();
  const module = await import("./trusted-types.ts");
  return { ...module, created, rules };
}

beforeEach(() => vi.unstubAllGlobals());
afterEach(() => vi.unstubAllGlobals());

test.each(PAGES)(
  "on %s the colouring worker's URL passes through the clauseo-sanitize policy",
  async (page) => {
    const { scriptUrl, created } = await load(page, true);
    const href = new URL(WORKER, page).href;
    expect(scriptUrl(WORKER)).toEqual({ trusted: href });
    expect(scriptUrl(href)).toEqual({ trusted: href });
    expect(scriptUrl(WORKER)).toEqual({ trusted: href });
    // Created once, on first use, however many workers start.
    expect(created).toEqual(["clauseo-sanitize"]);
  },
);

test.each(PAGES)("on %s every other script URL is refused", async (page) => {
  const { scriptUrl, rules } = await load(page, true);
  const host = new URL(page).host;
  const worker = new URL(WORKER, page);
  for (const url of [
    "/assets/worker-abc.js",
    "./worker.js",
    "/assets/../../x.js",
    "../../../../etc/passwd",
    `${worker.href}&extra`,
    `${worker.pathname}x`,
    `blob:${new URL(page).origin}/0b5c4a3e-0000-4000-8000-000000000000`,
    "data:text/javascript,postMessage(1)",
    "javascript:postMessage(1)",
    "https://evil.example/worker.js",
    "//evil.example/worker.js",
    `http://${host}.evil.example/worker.js`,
    `${new URL(page).protocol}//user:pass@${host}/worker.js`,
    page.startsWith("https:") ? `http://${host}/worker.js` : `https://${host}/worker.js`,
    "http://localhost:1421/worker.js",
    "file:///etc/passwd",
  ]) {
    expect(() => scriptUrl(url), url).toThrow(TypeError);
  }
  // The policy's own rule refuses them too, not only scriptUrl's check before it.
  scriptUrl(WORKER);
  expect(() => rules.createScriptURL?.("/assets/other.js")).toThrow(TypeError);
  expect(() => rules.createScriptURL?.("data:text/javascript,1")).toThrow(TypeError);
});

test("an engine without Trusted Types gets the checked URL itself, and the same refusals", async () => {
  const { scriptUrl } = await load("tauri://localhost/", false);
  expect(scriptUrl(WORKER)).toBe(new URL(WORKER, "tauri://localhost/").href);
  expect(() => scriptUrl("/assets/other.js")).toThrow(TypeError);
  expect(() => scriptUrl("data:text/javascript,1")).toThrow(TypeError);
  expect(() => scriptUrl("https://evil.example/w.js")).toThrow(TypeError);
});

test("a policy the engine refuses to create leaves the URL unstarted", async () => {
  vi.stubGlobal("location", { href: "tauri://localhost/", protocol: "tauri:", host: "localhost" });
  vi.stubGlobal("trustedTypes", {
    createPolicy() {
      throw new TypeError("duplicate policy");
    },
  });
  vi.resetModules();
  const { scriptUrl } = await import("./trusted-types.ts");
  expect(() => scriptUrl(WORKER)).toThrow(TypeError);
});
