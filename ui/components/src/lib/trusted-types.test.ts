import { afterEach, beforeEach, expect, test, vi } from "vitest";

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
  if (withTypes) {
    vi.stubGlobal("trustedTypes", {
      createPolicy(name: string, rules: { createScriptURL: (url: string) => string }) {
        if (created.includes(name)) throw new TypeError("duplicate policy");
        created.push(name);
        return { createScriptURL: (input: string) => ({ trusted: rules.createScriptURL(input) }) };
      },
    });
  }
  vi.resetModules();
  const module = await import("./trusted-types.ts");
  return { ...module, created };
}

beforeEach(() => vi.unstubAllGlobals());
afterEach(() => vi.unstubAllGlobals());

test.each(PAGES)(
  "on %s the page's own script URLs pass through the clauseo-sanitize policy",
  async (page) => {
    const { scriptUrl, created } = await load(page, true);
    expect(scriptUrl("/assets/worker-abc.js")).toEqual({
      trusted: new URL("/assets/worker-abc.js", page).href,
    });
    expect(
      scriptUrl(new URL("/src/markdown/highlight/worker.ts?worker_file&type=module", page).href),
    ).toEqual({
      trusted: new URL("/src/markdown/highlight/worker.ts?worker_file&type=module", page).href,
    });
    expect(scriptUrl("./worker.js")).toEqual({ trusted: new URL("./worker.js", page).href });
    // Created once, on first use, however many workers start.
    expect(created).toEqual(["clauseo-sanitize"]);
  },
);

test.each(PAGES)("on %s every other script URL is refused", async (page) => {
  const { scriptUrl } = await load(page, true);
  const host = new URL(page).host;
  for (const url of [
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
});

test("an engine without Trusted Types gets the checked URL itself, and the same refusals", async () => {
  const { scriptUrl } = await load("tauri://localhost/", false);
  expect(scriptUrl("/assets/worker.js")).toBe("tauri://localhost/assets/worker.js");
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
  expect(() => scriptUrl("/assets/worker.js")).toThrow(TypeError);
});
