import { build, createServer, resolveConfig, type Rolldown } from "vite";
import { afterEach, expect, test, vi } from "vitest";
import tauriConf from "../../../crates/desktop/tauri.conf.json" with { type: "json" };
import { devCsp, metaCsp, unwrapPreloads } from "../vite.config.ts";

/** A test that runs a whole Vite build: several seconds on a CI runner, past vitest's 5 s default. */
const BUILD = { timeout: 60_000 };

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
// chunk, and the entity table in its own, imported with a bare import() from the chunk that decodes entities: neither
// goes through Vite's preload helper, whose call would wrap it in an arrow (vite.config.ts's bareDynamicImports).
test(
  "the release build keeps the perf module and the entity table in lazy chunks, each imported without a preload",
  BUILD,
  async () => {
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
    const entities = chunks.find((c) => c.fileName.startsWith("assets/character-entities-"));
    expect(entities).toBeDefined();
    expect(entities!.isDynamicEntry).toBe(true);
    const decoders = chunks.filter((c) => /import\(`\.\/character-entities-/.test(c.code));
    expect(decoders.length).toBe(1);
    expect(decoders[0]!.code).toMatch(/[^>]import\(`\.\/character-entities-[\w-]+\.js`\)\.then\(/);
    expect(entry!.code).not.toContain("startStream");
  },
);

/** A build in memory under `mode`, its chunks, its other files and its index.html. */
async function builtWith(mode?: string) {
  // as `vite build` runs it: vitest's NODE_ENV=test would make the build a development one (import.meta.env.DEV)
  vi.stubEnv("NODE_ENV", "production");
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
  return { chunks, files, html: String(page?.source ?? "") };
}

// the page's charset first, inside the HTML prescan's 1024 bytes, then the CSP before any script, style or link
const META =
  /^<!doctype html>\s*<html[^>]*>\s*<head>\s*<meta charset="UTF-8"\s*\/?>\s*<meta http-equiv="Content-Security-Policy" content="([^"]*)">/i;
const unescape = (html: string) => html.replaceAll("&#39;", "'").replaceAll("&quot;", '"');

afterEach(() => {
  vi.unstubAllEnvs();
});

test("the meta CSP is the production CSP less frame-ancestors, which a meta tag may not carry", () => {
  const production = tauriConf.app.security.csp;
  expect(production).toContain("frame-ancestors 'none'");
  const meta = metaCsp(production);
  expect(meta).not.toContain("frame-ancestors");
  expect(meta.startsWith("default-src 'self'; script-src 'self'; style-src 'self'; ")).toBe(true);
  expect(meta).toContain(
    "; base-uri 'none'; form-action 'none'; require-trusted-types-for 'script'; ",
  );
  expect(
    meta.endsWith("; require-trusted-types-for 'script'; trusted-types clauseo-sanitize"),
  ).toBe(true);
  // one directive fewer than the production policy, which has 17
  expect(production.split(";").length).toBe(17);
  expect(meta.split("; ").length).toBe(16);
});

/** What only the copy review page holds: none of it may reach the release build. */
const REVIEW_ONLY = ["U1-01", "copy-approvals"];

/** The build's hooks for checks by hand and axe-core's chunk, where it has them. */
function handCheckChunks(chunks: Rolldown.OutputChunk[]) {
  return {
    hooks: chunks.filter(
      (c) => c.fileName.startsWith("assets/dev-hooks-") || c.code.includes("__clauseoAxe"),
    ),
    axe: chunks.filter(
      (c) =>
        c.fileName.startsWith("assets/axe") ||
        c.code.includes("axe.run") ||
        c.moduleIds.some((id) => id.includes("/axe-core/")),
    ),
  };
}

// The release build, without VITE_CLAUSEO_HAND_CHECKS: the room and its stylesheet, the policy as its head's first
// element after the charset, and neither the copy review page, the hooks nor axe-core anywhere in it.
test(
  "the release build is the room under the meta CSP, with no review page, hooks or axe-core chunk",
  BUILD,
  async () => {
    const { html, chunks, files } = await builtWith();
    expect(unescape(META.exec(html)?.[1] ?? "")).toBe(metaCsp(tauriConf.app.security.csp));
    expect(chunks.some((c) => c.moduleIds.some((id) => id.includes("/src/room/")))).toBe(true);
    expect(chunks.some((c) => c.code.includes("room-window"))).toBe(true);
    expect(files.some((f) => f.fileName.endsWith(".css"))).toBe(true);
    for (const chunk of chunks) {
      for (const word of REVIEW_ONLY)
        expect(chunk.code, `${chunk.fileName}: ${word}`).not.toContain(word);
      const strays = chunk.moduleIds.filter((id) => id.includes("/src/review/"));
      expect(strays, chunk.fileName).toEqual([]);
    }
    expect(handCheckChunks(chunks)).toEqual({ hooks: [], axe: [] });
  },
);

// With VITE_CLAUSEO_HAND_CHECKS=1, the e2e build: the hooks (room/dev-hooks.ts) and axe-core in chunks of their
// own that the page doesn't load at start.
test(
  "the build with VITE_CLAUSEO_HAND_CHECKS=1 keeps its hooks and axe-core in chunks of their own",
  BUILD,
  async () => {
    vi.stubEnv("VITE_CLAUSEO_HAND_CHECKS", "1");
    const { html, chunks } = await builtWith();
    const { hooks, axe } = handCheckChunks(chunks);
    expect(hooks.map((c) => c.isDynamicEntry)).toEqual([true]);
    expect(axe.map((c) => c.isDynamicEntry)).toEqual([true]);
    const initial = new Set(
      [...html.matchAll(/(?:src|href)="\/(assets\/[^"]+\.js)"/g)].map((m) => m[1]),
    );
    expect(initial.has(hooks[0]?.fileName ?? "")).toBe(false);
    expect(initial.has(axe[0]?.fileName ?? "")).toBe(false);
  },
);

test(
  "the probe build mounts the room over FakeTransport from room/probe-main.tsx, under the meta CSP, with the hooks",
  BUILD,
  async () => {
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
    expect(handCheckChunks(chunks).axe.length).toBe(1);
  },
);

test("unwrapPreloads unwraps only the listed chunks, bodies with brackets and strings included", () => {
  const wrapped = (body: string) => `__vitePreload(() => ${body}, __VITE_PRELOAD__)`;
  const entities =
    'import("./character-entities-x1.js").then(function (m) { t = m[")"]; }, function (e) { throw e; })';
  const perf = 'import("./perf-x2.js")';
  const other = 'import("./html-x3.js")';
  expect(
    unwrapPreloads(`a = ${wrapped(entities)}; b = ${wrapped(perf)}; c = ${wrapped(other)};`),
  ).toBe(`a = ${entities}; b = ${perf}; c = ${wrapped(other)};`);
  expect(() => unwrapPreloads('__vitePreload(() => import("./perf-x.js")')).toThrow(
    /cannot unwrap/,
  );
});
