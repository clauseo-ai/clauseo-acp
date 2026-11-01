import { build, createServer, resolveConfig, type Rolldown } from "vite";
import { afterEach, expect, test, vi } from "vitest";
import tauriConf from "../../../crates/desktop/tauri.conf.json" with { type: "json" };
import { devCsp, metaCsp } from "../vite.config.ts";

// The React Compiler skips a function it cannot compile instead of failing the build, so a broken
// Babel or compiler setup would only lose memoization. This loads the real vite.config.ts and
// transforms App.tsx the way the dev server does, so it fails if the compiler is not wired in.
// (`memo_cache_sentinel` in a bundle proves nothing: React's own runtime defines it.) The path is
// relative to ui/app, where `pnpm -C ui/app run test` runs vitest.
test("vite.config.ts builds App.tsx with the React Compiler", async () => {
  const server = await createServer({
    configFile: "vite.config.ts",
    server: { middlewareMode: true, watch: null },
    logLevel: "silent",
  });
  try {
    const result = await server.transformRequest("/src/App.tsx");
    // The dev server rewrites `react/compiler-runtime` to its prebundled `react_compiler-runtime`.
    expect(result?.code).toMatch(/react[/_]compiler-runtime/);
  } finally {
    await server.close();
  }
});

test("the dev CSP is the production CSP plus inline script and style and Vite's websocket", () => {
  const production = tauriConf.app.security.csp;
  const dev = devCsp(production, 1420);
  expect(dev).toContain("script-src 'self' 'unsafe-inline';");
  expect(dev).toContain("style-src 'self' 'unsafe-inline';");
  expect(dev).toContain("require-trusted-types-for 'script'");
  // Every production directive survives, in order, with only those additions.
  const strip = (policy: string) =>
    policy.replaceAll(" 'unsafe-inline'", "").replace(" ws://localhost:1420", "");
  expect(strip(dev)).toBe(production.replaceAll(/\s*;\s*/g, "; "));
});

test("the dev CSP allows Vite's websocket on its own port and no other", () => {
  const dev = devCsp(tauriConf.app.security.csp, 51234);
  const connect = dev.split("; ").find((directive) => directive.startsWith("connect-src"));
  expect(connect?.endsWith(" ws://localhost:51234")).toBe(true);
  expect(dev).not.toContain("localhost:*");
});

test("the dev CSP refuses a production CSP without a directive it adds to", () => {
  expect(() => devCsp("default-src 'self'; style-src 'self'; connect-src ipc:", 1420)).toThrow(
    /no script-src/,
  );
});

test("the build's only input is index.html, so the dev-only gallery stays out of the shipped bundle", async () => {
  const config = await resolveConfig({ configFile: "vite.config.ts", logLevel: "silent" }, "build");
  expect(config.build.rolldownOptions.input).toBeUndefined();
  expect(config.build.assetsInlineLimit).toBe(0);
});

// The perf module (src/perf/) loads only in the e2e and marks builds; a release page must not pay for it. This builds
// the app in memory and checks the perf code sits in its own chunk, imported with a bare import() from the entry
// chunk, and that Vite's preload helper (about 1.8 KB, which vite.config.ts's bareDynamicImports unwraps) is gone.
test("the release build keeps the perf module in a lazy chunk, with no preload helper in the entry", async () => {
  const out = (await build({
    configFile: "vite.config.ts",
    logLevel: "silent",
    build: { write: false },
  })) as Rolldown.RolldownOutput | Rolldown.RolldownOutput[];
  const chunks = (Array.isArray(out) ? out : [out])
    .flatMap((o) => o.output)
    .filter((c): c is Rolldown.OutputChunk => c.type === "chunk");
  const entry = chunks.find((c) => c.isEntry);
  const perf = chunks.find((c) => c.fileName.startsWith("assets/perf-"));
  expect(entry).toBeDefined();
  expect(perf).toBeDefined();
  expect(perf!.isDynamicEntry).toBe(true);
  expect(entry!.code).toMatch(/[^>]import\(`\.\/perf-[\w-]+\.js`\)/);
  expect(entry!.code).not.toContain("vite:preloadError");
  expect(entry!.code).not.toContain("startStream");
});

/** A build in memory under `mode`, its chunks and its index.html. */
async function builtWith(mode?: string) {
  const out = (await build({
    configFile: "vite.config.ts",
    logLevel: "silent",
    ...(mode === undefined ? {} : { mode }),
    build: { write: false },
  })) as Rolldown.RolldownOutput | Rolldown.RolldownOutput[];
  const files = (Array.isArray(out) ? out : [out]).flatMap((o) => o.output);
  const chunks = files.filter((c): c is Rolldown.OutputChunk => c.type === "chunk");
  const page = files.find(
    (f): f is Rolldown.OutputAsset => f.type === "asset" && f.fileName === "index.html",
  );
  return { chunks, html: String(page?.source ?? "") };
}

const META =
  /^<!doctype html>\s*<html[^>]*>\s*<head>\s*<meta http-equiv="Content-Security-Policy" content="([^"]*)">/i;
const unescape = (html: string) => html.replaceAll("&#39;", "'").replaceAll("&quot;", '"');

afterEach(() => {
  vi.unstubAllEnvs();
});

test("the meta CSP is the production CSP less frame-ancestors, which a meta tag may not carry", () => {
  const production = tauriConf.app.security.csp;
  expect(production).toContain("frame-ancestors");
  const meta = metaCsp(production);
  expect(meta).not.toContain("frame-ancestors");
  expect(meta).toBe(
    production
      .split(";")
      .map((d) => d.trim())
      .filter((d) => d !== "" && !d.startsWith("frame-ancestors"))
      .join("; "),
  );
});

test("the default build's page carries no meta CSP: Tauri's header is its only policy", async () => {
  const { html } = await builtWith();
  expect(html).not.toContain("Content-Security-Policy");
});

// The room's build (VITE_CLAUSEO_WINDOW=room) carries the policy as the first thing in its head, and keeps the hooks
// for checks by hand (room/dev-hooks.ts) and axe-core in chunks of their own that no release page loads at start.
test("the room build's page starts its head with the meta CSP; its hooks and axe-core are chunks of their own", async () => {
  vi.stubEnv("VITE_CLAUSEO_WINDOW", "room");
  const { html, chunks } = await builtWith();
  expect(unescape(META.exec(html)?.[1] ?? "")).toBe(metaCsp(tauriConf.app.security.csp));
  const hooks = chunks.find((c) => c.fileName.startsWith("assets/dev-hooks-"));
  expect(hooks?.isDynamicEntry).toBe(true);
  const carrying = chunks.filter((c) => c.code.includes("__clauseoAxe")).map((c) => c.fileName);
  expect(carrying).toEqual([hooks?.fileName]);
  const axe = chunks.find((c) => c.code.includes("axe.run") || c.fileName.startsWith("assets/axe"));
  expect(axe?.isDynamicEntry).toBe(true);
  const initial = new Set(
    [...html.matchAll(/(?:src|href)="\/(assets\/[^"]+\.js)"/g)].map((m) => m[1]),
  );
  expect(initial.has(hooks?.fileName ?? "")).toBe(false);
  expect(initial.has(axe?.fileName ?? "")).toBe(false);
});

test("the probe build mounts the room over FakeTransport from room/probe-main.tsx, under the meta CSP", async () => {
  const config = await resolveConfig(
    { configFile: "vite.config.ts", logLevel: "silent", mode: "probe" },
    "build",
  );
  expect(config.build.outDir).toBe("dist-probe");
  const { html, chunks } = await builtWith("probe");
  expect(unescape(META.exec(html)?.[1] ?? "")).toBe(metaCsp(tauriConf.app.security.csp));
  const entry = chunks.find((c) => c.isEntry);
  const modules = entry?.moduleIds ?? [];
  expect(modules.some((id) => id.endsWith("/src/room/probe-main.tsx"))).toBe(true);
  expect(modules.some((id) => id.endsWith("/src/main.tsx"))).toBe(false);
});
